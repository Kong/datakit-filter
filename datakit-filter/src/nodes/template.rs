use proxy_wasm::{traits::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::any::Any;
//use handlebars::Handlebars;

use crate::data::{Payload, State};
use crate::nodes::Connections;
use crate::nodes::{Node, NodeConfig};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TemplateConfig {
    connections: Connections,
}

impl NodeConfig for TemplateConfig {
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
        "template"
    }

    fn from_map(_bt: BTreeMap<String, Value>, connections: Connections) -> Box<dyn NodeConfig>
    where
        Self: Sized,
    {
        Box::new(TemplateConfig {
            connections,
        })
    }
}

#[derive(Clone)]
pub struct Template {
    config: TemplateConfig
}

impl Node for Template {
    fn new(config: &Box<dyn NodeConfig>) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<TemplateConfig>() {
            Some(cc) => Box::new(Template { config: cc.clone() }),
            None => panic!("incompatible NodeConfig"),
        }
    }

    fn clone_dyn(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }

    fn get_name(&self) -> &str {
        &self.config.connections.name
    }

    fn run(
        &mut self,
        ctx: &dyn HttpContext,
        inputs: Vec<&Payload>,
    ) -> State {
        log::info!("Template: run - inputs: {:?}", inputs);

        ctx.get_http_request_headers();

        State::Done(None)
    }
}
