use proxy_wasm::traits::*;
use serde::Deserialize;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;

use crate::data;
use crate::data::{Payload, State, State::*};
use crate::nodes::Connections;
use crate::nodes::{get_config_value, Node, NodeConfig};

#[derive(Deserialize, Clone, Debug)]
pub struct ResponseConfig {
    connections: Connections,

    // FIXME: the optional ones should be Option,
    // but we're not really serializing this for now, just deserializing...
    status: u32,
}

impl NodeConfig for ResponseConfig {
    fn get_connections(&self) -> &Connections {
        &self.connections
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_dyn(&self) -> Box<dyn NodeConfig> {
        Box::new(self.clone())
    }

    fn get_node_type(&self) -> &'static str {
        "response"
    }

    fn from_map(bt: BTreeMap<String, Value>, connections: Connections) -> Box<dyn NodeConfig>
    where
        Self: Sized,
    {
        Box::new(ResponseConfig {
            connections,
            status: get_config_value(&bt, "status", 200),
        })
    }
}

#[derive(Clone)]
pub struct Response {
    config: ResponseConfig,
}

impl Node for Response {
    fn new_box(config: &Box<dyn NodeConfig>) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<ResponseConfig>() {
            Some(cc) => Box::new(Response { config: cc.clone() }),
            None => panic!("incompatible NodeConfig"),
        }
    }

    fn get_name(&self) -> &str {
        &self.config.connections.name
    }

    fn run(&mut self, ctx: &dyn HttpContext, inputs: Vec<Option<&Payload>>) -> State {
        let body = inputs.first().unwrap_or(&None);
        let headers = inputs.get(1).unwrap_or(&None);

        ctx.send_http_response(
            self.config.status,
            data::to_pwm_headers(*headers),
            data::to_pwm_body(*body).as_deref(),
        );

        Done(None)
    }
}
