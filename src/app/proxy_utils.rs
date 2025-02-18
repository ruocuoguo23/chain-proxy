use bytes::Bytes;
use log::{error, info};
use pingora_proxy::Session;
use pingora::{
    Error,
    Result
};

use crate::app::proxy_base::ProxyCtx;

pub async fn request_body_filter(
    body: &mut Option<Bytes>,
    ctx: &mut ProxyCtx,
) -> Result<()> {
    if let Some(buf) = body {
        ctx.request_body.extend_from_slice(buf);
    }
    Ok(())
}

pub fn upstream_response_body_filter(
    body: &mut Option<Bytes>,
    ctx: &mut ProxyCtx,
) {
    if let Some(buf) = body {
        ctx.response_body.extend_from_slice(buf);
    }
}

pub async fn logging(
    session: &mut Session,
    e: Option<&Error>,
    ctx: &mut ProxyCtx,
) {
    let req_body = String::from_utf8_lossy(&ctx.request_body);
    let resp_body = String::from_utf8_lossy(&ctx.response_body);
    let response_code = session
        .response_written()
        .map_or(0, |resp| resp.status.as_u16());

    if let Some(e) = e {
        error!(
                "Request: request body={}\nResponse: status={}, response body={}\nError: {}",
                req_body,
                response_code,
                resp_body,
                e
            );
    } else {
        info!(
                "Request: request body={}\nResponse: status={}, response body={}",
                req_body,
                response_code,
                resp_body
            );
    }
}