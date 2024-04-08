use handlebars::Handlebars;
use proxy_wasm::traits::*;
use serde::Deserialize;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;

use crate::data::{Payload, State};
use crate::nodes::Connections;
use crate::nodes::{get_config_value, Node, NodeConfig, NodeFactory};

#[derive(Deserialize, Clone, Debug)]
pub struct TemplateConfig {
    connections: Connections,

    template: String,
    content_type: String,
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
}

#[derive(Clone)]
pub struct Template<'a> {
    config: TemplateConfig,
    handlebars: Handlebars<'a>,
}

impl Template<'_> {
    fn new(config: TemplateConfig) -> Self {
        let mut hb = Handlebars::new();

        match hb.register_template_string("template", &config.template) {
            Ok(()) => {}
            Err(err) => {
                log::error!("template: error registering template: {}", err);
            }
        }

        Template {
            config,
            handlebars: hb,
        }
    }
}

impl Node for Template<'_> {
    fn get_name(&self) -> &str {
        &self.config.connections.name
    }

    fn run(&mut self, _ctx: &dyn HttpContext, inputs: Vec<Option<&Payload>>) -> State {
        log::debug!("template: run - inputs: {:?}", inputs);

        let mut vs = Vec::new();
        let mut data = BTreeMap::new();

        for (input_name, input) in self.config.connections.each_input().zip(inputs.iter()) {
            match input {
                Some(Payload::Json(value)) => {
                    data.insert(input_name, value);
                }
                Some(Payload::Raw(vec_bytes)) => {
                    match std::str::from_utf8(vec_bytes) {
                        Ok(s) => {
                            let v = serde_json::to_value::<String>(s.into())
                                .expect("valid UTF-8 string");
                            vs.push((input_name, v));
                        }
                        Err(err) => {
                            log::error!("template: input string is not valid UTF-8: {}", err);
                        }
                    };
                }
                None => {}
            }
        }

        for (input_name, v) in vs.iter() {
            data.insert(input_name, v);
        }

        State::Done(match self.handlebars.render("template", &data) {
            Ok(output) => {
                log::error!("output: {}", output);
                Payload::from_bytes(output.into(), Some(&self.config.content_type))
            }
            Err(err) => {
                log::error!("template: error rendering template: {}", err);
                None
            }
        })
    }
}

pub struct TemplateFactory {}

impl NodeFactory for TemplateFactory {
    fn config_from_map(
        &self,
        bt: BTreeMap<String, Value>,
        connections: Connections,
    ) -> Box<dyn NodeConfig> {
        Box::new(TemplateConfig {
            connections,
            template: get_config_value(&bt, "template", String::from("")),
            content_type: get_config_value(&bt, "content_type", String::from("application/json")),
        })
    }

    fn new_box(&self, config: &Box<dyn NodeConfig>) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<TemplateConfig>() {
            Some(cc) => Box::new(Template::new(cc.clone())),
            None => panic!("incompatible NodeConfig"),
        }
    }
}
