use pingora::protocols::ALPN;
use pingora::protocols::l4::ext::TcpKeepalive;
use pingora::upstreams::peer::PeerOptions;
use std::collections::BTreeMap;
use std::time::Duration;

/// Default peer options to be used on every upstream connection
pub const DEFAULT_PEER_OPTIONS: PeerOptions = PeerOptions {
    verify_hostname: true,
    read_timeout: Some(Duration::from_secs(30)),
    connection_timeout: Some(Duration::from_secs(30)),
    tcp_recv_buf: Some(512 * 1024),
    tcp_keepalive: Some(TcpKeepalive {
        count: 5,
        interval: Duration::from_secs(10),
        idle: Duration::from_secs(30),
    }),
    bind_to: None,
    total_connection_timeout: Some(Duration::from_secs(5)),
    idle_timeout: None,
    write_timeout: Some(Duration::from_secs(5)),
    verify_cert: false,
    alternative_cn: None,
    alpn: ALPN::H1,
    ca: None,
    h2_ping_interval: None,
    max_h2_streams: 5,
    extra_proxy_headers: BTreeMap::new(),
    curves: None,
    second_keyshare: true, // default true and noop when not using PQ curves
    tracer: None,
    dscp: None,
    tcp_fast_open: false,
    custom_l4: None,
};