use proxy_wasm::traits::*;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;

use crate::config::get_config_value;
use crate::data;
use crate::data::{Input, Payload, Phase, State, State::*};
use crate::nodes::{Node, NodeConfig, NodeFactory};

#[derive(Debug)]
pub struct ResponseConfig {
    name: String,
    status: Option<u32>,
    warn_headers_sent: AtomicBool,
}

impl Clone for ResponseConfig {
    fn clone(&self) -> ResponseConfig {
        ResponseConfig {
            name: self.name.clone(),
            status: self.status,
            warn_headers_sent: AtomicBool::new(self.warn_headers_sent.load(Relaxed)),
        }
    }
}

impl NodeConfig for ResponseConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn default_outputs(&self) -> Option<Vec<String>> {
        Some(vec!["response_body".to_string()])
    }
}

#[derive(Clone)]
pub struct Response {
    config: ResponseConfig,
}

fn warn_headers_sent(config: &ResponseConfig, set_headers: bool) {
    let name = &config.name;
    let set_status = config.status.is_some();

    if set_status || set_headers {
        let what = if set_headers && set_status {
            "status or headers"
        } else if set_status {
            "status"
        } else {
            "headers"
        };
        log::warn!(
            "response: node '{name}' cannot set {what} when processing response body, \
                   headers already sent; set 'warn_headers_sent' to false \
                   to silence this warning",
        );
    }
    config.warn_headers_sent.store(false, Relaxed);
}

impl Node for Response {
    fn run(&self, ctx: &dyn HttpContext, input: &Input) -> State {
        let config = &self.config;
        let body = input.data.first().unwrap_or(&None).as_deref();
        let headers = input.data.get(1).unwrap_or(&None).as_deref();

        let mut headers_vec = data::to_pwm_headers(headers);

        if let Some(payload) = body {
            if let Some(content_type) = payload.content_type() {
                headers_vec.push(("Content-Type", content_type));
            }
        }

        let body_slice = match data::to_pwm_body(body) {
            Ok(slice) => slice,
            Err(e) => return Fail(Some(Payload::Error(e))),
        };

        if input.phase == Phase::HttpResponseBody {
            if config.warn_headers_sent.load(Relaxed) {
                warn_headers_sent(config, headers.is_some());
            }

            if let Some(b) = body_slice {
                ctx.set_http_response_body(0, b.len(), &b);
            }
        } else {
            let status = config.status.unwrap_or(200);
            ctx.send_http_response(status, headers_vec, body_slice.as_deref());
        }

        Done(None)
    }
}

pub struct ResponseFactory {}

impl NodeFactory for ResponseFactory {
    fn new_config(
        &self,
        name: &str,
        _inputs: &[String],
        bt: &BTreeMap<String, Value>,
    ) -> Result<Box<dyn NodeConfig>, String> {
        Ok(Box::new(ResponseConfig {
            name: name.to_string(),
            status: get_config_value(bt, "status"),
            warn_headers_sent: AtomicBool::new(
                get_config_value(bt, "warn_headers_sent").unwrap_or(true),
            ),
        }))
    }

    fn new_node(&self, config: &dyn NodeConfig) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<ResponseConfig>() {
            Some(cc) => Box::new(Response { config: cc.clone() }),
            None => panic!("incompatible NodeConfig"),
        }
    }
}
