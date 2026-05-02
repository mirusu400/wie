use alloc::{boxed::Box, string::String, vec, vec::Vec};

use wipi_types::wipic::WIPICWord;

use wie_backend::Network;
use wie_util::{Result, WieError, read_null_terminated_string_bytes};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};

const M_E_NONE: i32 = 0;
const M_E_INVALID: i32 = -9;

const POLL_INTERVAL_MS: u64 = 30;

pub async fn connect(context: &mut dyn WIPICContext, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_netConnect({cb:#x}, {param:#x})");

    struct ConnectCallback {
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ConnectCallback {
        #[tracing::instrument(name = "MC_netConnect.cb", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            context.system().sleep(1).await;
            // No real bearer to bring up — the host always has IP. Report success.
            context.call_function(self.cb, &[M_E_NONE as u32, self.param]).await?;
            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(ConnectCallback { cb, param }))?;
    Ok(0)
}

pub async fn close(_context: &mut dyn WIPICContext) -> Result<()> {
    tracing::debug!("MC_netClose()");
    Ok(())
}

pub async fn socket(context: &mut dyn WIPICContext, domain: i32, socket_type: i32) -> Result<i32> {
    // domain: MC_AF_INET=2; type: MC_SOCKET_STREAM=1, MC_SOCKET_DGRAM=2.
    // UDP isn't wired through Network::* yet; keep STREAM only.
    if domain != 2 || socket_type != 1 {
        tracing::warn!("MC_netSocket unsupported domain={domain} type={socket_type}");
        return Ok(M_E_INVALID);
    }
    let fd = context.system().network().socket_create();
    tracing::debug!("MC_netSocket(domain={domain}, type={socket_type}) -> fd {fd}");
    Ok(fd)
}

pub async fn socket_connect(context: &mut dyn WIPICContext, fd: i32, addr: i32, port: i32, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    let addr_u32 = addr as u32;
    let port_u16 = (port as u32 & 0xffff) as u16;
    tracing::debug!("MC_netSocketConnect(fd={fd}, addr={addr_u32:#x}, port={port_u16}, cb={cb:#x})");

    let net = context.system().network().clone();
    if let Err(code) = net.socket_connect_start(fd, addr_u32, port_u16) {
        return Ok(code);
    }

    struct ConnectCb {
        net: Network,
        fd: i32,
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ConnectCb {
        #[tracing::instrument(name = "MC_netSocketConnect.cb", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            loop {
                match self.net.socket_connect_poll(self.fd) {
                    Some(Ok(())) => {
                        context.call_function(self.cb, &[self.fd as u32, M_E_NONE as u32, self.param]).await?;
                        break;
                    }
                    Some(Err(code)) => {
                        context.call_function(self.cb, &[self.fd as u32, code as u32, self.param]).await?;
                        break;
                    }
                    None => context.system().sleep(POLL_INTERVAL_MS).await,
                }
            }
            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(ConnectCb { net, fd, cb, param }))?;
    Ok(0)
}

pub async fn socket_write(context: &mut dyn WIPICContext, fd: i32, buf: WIPICWord, len: i32) -> Result<i32> {
    if len < 0 {
        return Ok(M_E_INVALID);
    }
    let mut data = vec![0u8; len as usize];
    if !data.is_empty() {
        context.read_bytes(buf, &mut data)?;
    }
    let net = context.system().network().clone();
    let result = match net.socket_write(fd, &data) {
        Ok(n) => n as i32,
        Err(code) => code,
    };
    tracing::trace!("MC_netSocketWrite(fd={fd}, len={len}) -> {result}");
    Ok(result)
}

pub async fn socket_read(context: &mut dyn WIPICContext, fd: i32, buf: WIPICWord, len: i32) -> Result<i32> {
    if len < 0 {
        return Ok(M_E_INVALID);
    }
    let mut data = vec![0u8; len as usize];
    let net = context.system().network().clone();
    let result = match net.socket_read(fd, &mut data) {
        Ok(n) => {
            if n > 0 {
                context.write_bytes(buf, &data[..n])?;
            }
            n as i32
        }
        Err(code) => code,
    };
    tracing::trace!("MC_netSocketRead(fd={fd}, len={len}) -> {result}");
    Ok(result)
}

pub async fn socket_close(context: &mut dyn WIPICContext, fd: i32) -> Result<i32> {
    tracing::debug!("MC_netSocketClose({fd})");
    Ok(match context.system().network().socket_close(fd) {
        Ok(()) => M_E_NONE,
        Err(code) => code,
    })
}

pub async fn set_read_cb(context: &mut dyn WIPICContext, fd: i32, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_netSetReadCB(fd={fd}, cb={cb:#x})");
    if cb == 0 {
        // Caller is clearing the callback. Nothing to spawn.
        return Ok(M_E_NONE);
    }
    let net = context.system().network().clone();

    struct ReadCb {
        net: Network,
        fd: i32,
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ReadCb {
        #[tracing::instrument(name = "MC_netSetReadCB.cb", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            loop {
                if self.net.socket_read_ready(self.fd) {
                    context.call_function(self.cb, &[self.fd as u32, M_E_NONE as u32, self.param]).await?;
                    break;
                }
                context.system().sleep(POLL_INTERVAL_MS).await;
            }
            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(ReadCb { net, fd, cb, param }))?;
    Ok(M_E_NONE)
}

pub async fn set_write_cb(context: &mut dyn WIPICContext, fd: i32, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_netSetWriteCB(fd={fd}, cb={cb:#x})");
    if cb == 0 {
        return Ok(M_E_NONE);
    }

    // Connected non-blocking sockets are essentially always writable until the
    // kernel buffer fills, which only happens for very large bursts. Schedule a
    // single fire-and-forget callback so the app can resume its write.
    struct WriteCb {
        fd: i32,
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for WriteCb {
        #[tracing::instrument(name = "MC_netSetWriteCB.cb", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            context.system().sleep(POLL_INTERVAL_MS).await;
            context.call_function(self.cb, &[self.fd as u32, M_E_NONE as u32, self.param]).await?;
            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(WriteCb { fd, cb, param }))?;
    Ok(M_E_NONE)
}

pub async fn get_host_addr(context: &mut dyn WIPICContext, _dnsserver: i32, hostname_ptr: WIPICWord, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    let raw = read_null_terminated_string_bytes(context, hostname_ptr)?;
    let host = String::from_utf8(raw).unwrap_or_default();
    tracing::debug!("MC_netGetHostAddr(host={host:?})");

    let net = context.system().network().clone();
    let dns_id = net.resolve_start(&host);

    struct DnsCb {
        net: Network,
        id: u32,
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for DnsCb {
        #[tracing::instrument(name = "MC_netGetHostAddr.cb", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            loop {
                match self.net.resolve_poll(self.id) {
                    Some(Some(ip)) => {
                        context.call_function(self.cb, &[ip, self.param]).await?;
                        break;
                    }
                    Some(None) => {
                        context.call_function(self.cb, &[0, self.param]).await?;
                        break;
                    }
                    None => context.system().sleep(POLL_INTERVAL_MS).await,
                }
            }
            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(DnsCb { net, id: dns_id, cb, param }))?;
    Ok(M_E_NONE)
}

pub async fn get_max_packet_length(_context: &mut dyn WIPICContext) -> Result<i32> {
    // Reasonable default for emulated TCP/UDP MTU.
    Ok(1500)
}
