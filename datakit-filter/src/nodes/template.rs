use proxy_wasm::{traits::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use handlebars::Handlebars;

use crate::data::{Payload, State};
use crate::nodes::Connections;
use crate::nodes::Node;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Template {
    connections: Connections,
}

impl Node for Template {
    fn run(
        &mut self,
        ctx: &dyn HttpContext,
        inputs: Vec<&Payload>,
    ) -> State {
        log::info!("Template: run - inputs: {:?}", inputs);

        ctx.get_http_request_headers();

        State::Done(None)
    }

    fn get_connections(&self) -> &Connections {
        &self.connections
    }

    fn clone_dyn(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }

    fn from_map(_bt: BTreeMap<String, Value>, connections: Connections) -> Box<dyn Node>
    where
        Self: Sized,
    {
        Box::new(Template {
            connections,
        })
    }
}
