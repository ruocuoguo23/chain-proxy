use std::sync::Mutex;

use lazy_static::lazy_static;
use prometheus::{GaugeVec, CounterVec, Opts, default_registry};

#[derive(Clone)]
pub struct Metrics {
    pub node_height_gauge: GaugeVec,

    // proxy result counter
    pub proxy_result_counter: CounterVec,
}

impl Metrics {
    pub fn new(namespace: &str) -> Self {
        let node_height_gauge = GaugeVec::new(
            Opts::new("node_height_gauge", "node height gauge").namespace(namespace),
            &["chain", "host"],
        )
            .unwrap();

        let proxy_result_counter = CounterVec::new(
            Opts::new("proxy_result_counter", "proxy result counter").namespace(namespace),
            &["chain", "host", "code", "method"],
        )
            .unwrap();

        Metrics {
            node_height_gauge,
            proxy_result_counter,
        }
    }

    pub fn register(self) -> Result<Self, prometheus::Error> {
        let registry = default_registry();
        registry.register(Box::new(self.node_height_gauge.clone()))?;
        registry.register(Box::new(self.proxy_result_counter.clone()))?;

        Ok(self)
    }

    pub fn set_node_height_gauge(&self, chain: &str, host: &str, height: u64) {
        self.node_height_gauge
            .with_label_values(&[chain, &host])
            .set(height as f64);
    }

    pub fn inc_proxy_result_counter(&self, chain: &str, host: &str, code: &str, method: &str) {
        self.proxy_result_counter
            .with_label_values(&[chain, host, code, method])
            .inc();
    }
}

lazy_static! {
    pub static ref METRICS: Mutex<Option<Metrics>> = Mutex::new(None);
}

pub fn init_metrics(system: &str) -> Result<(), prometheus::Error> {
    let metrics = Metrics::new(system).register()?;
    let mut metrics_lock = METRICS.lock().unwrap();
    *metrics_lock = Some(metrics);

    Ok(())
}

pub fn set_node_height_gauge(chain: &str, host: &str, height: u64) {
    let metrics_lock = METRICS.lock().unwrap();
    if let Some(metrics) = &*metrics_lock {
        metrics.set_node_height_gauge(chain, host, height);
    }
}

pub fn inc_proxy_result_counter(chain: &str, host: &str, code: &str, method: &str) {
    let metrics_lock = METRICS.lock().unwrap();
    if let Some(metrics) = &*metrics_lock {
        metrics.inc_proxy_result_counter(chain, host, code, method);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics() {
        // Initialize metrics
        init_metrics("wallet").unwrap();

        // Set a test value
        set_node_height_gauge("test_chain", "test_host", 42);
        inc_proxy_result_counter("test_chain", "test_host", "200", "GET");
        inc_proxy_result_counter("test_chain", "test_host", "404", "POST");
        inc_proxy_result_counter("test_chain", "test_host", "500", "PUT");

        // Check if the value is set correctly
        let metric_families = prometheus::gather();
        let metric = metric_families
            .iter()
            .find(|m| m.get_name() == "wallet_node_height_gauge")
            .unwrap();

        let metric = metric.get_metric();
        assert_eq!(metric.len(), 1);

        let proxy_result_counter = metric_families
            .iter()
            .find(|m| m.get_name() == "wallet_proxy_result_counter")
            .unwrap();
        let proxy_result_counter = proxy_result_counter.get_metric();
        assert_eq!(proxy_result_counter.len(), 3);
    }
}