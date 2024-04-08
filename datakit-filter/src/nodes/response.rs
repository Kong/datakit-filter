use proxy_wasm::traits::*;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;

use crate::config::get_config_value;
use crate::data;
use crate::data::{Payload, State, State::*};
use crate::nodes::{Node, NodeConfig, NodeFactory};

#[derive(Clone, Debug)]
pub struct ResponseConfig {
    // FIXME: the optional ones should be Option,
    // but we're not really serializing this for now, just deserializing...
    status: u32,
}

impl NodeConfig for ResponseConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
pub struct Response {
    config: ResponseConfig,
}

impl Node for Response {
    fn run(&mut self, ctx: &dyn HttpContext, inputs: Vec<Option<&Payload>>) -> State {
        let body = inputs.first().unwrap_or(&None);
        let headers = inputs.get(1).unwrap_or(&None);

        let mut headers_vec = data::to_pwm_headers(*headers);

        if let Some(payload) = body {
            if let Some(content_type) = payload.content_type() {
                headers_vec.push(("Content-Type", content_type));
            }
        }

        ctx.send_http_response(
            self.config.status,
            headers_vec,
            data::to_pwm_body(*body).as_deref(),
        );

        Done(None)
    }
}

pub struct ResponseFactory {}

impl NodeFactory for ResponseFactory {
    fn new_config(
        &self,
        _name: &str,
        _inputs: &[String],
        bt: &BTreeMap<String, Value>,
    ) -> Result<Box<dyn NodeConfig>, String> {
        Ok(Box::new(ResponseConfig {
            status: get_config_value(bt, "status", 200),
        }))
    }

    fn new_node(&self, config: &dyn NodeConfig) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<ResponseConfig>() {
            Some(cc) => Box::new(Response { config: cc.clone() }),
            None => panic!("incompatible NodeConfig"),
        }
    }
}
