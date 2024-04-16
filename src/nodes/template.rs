use handlebars::Handlebars;
use proxy_wasm::traits::*;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;

use crate::config::get_config_value;
use crate::data::{Payload, State};
use crate::nodes::{FilterPhase, Node, NodeConfig, NodeFactory};

#[derive(Clone, Debug)]
pub struct TemplateConfig {
    template: String,
    content_type: String,
    inputs: Vec<String>,
}

impl NodeConfig for TemplateConfig {
    fn as_any(&self) -> &dyn Any {
        self
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
                log::error!("template: error registering template: {err}");
            }
        }

        Template {
            config,
            handlebars: hb,
        }
    }
}

impl Node for Template<'_> {
    fn run(&self, _ctx: &dyn HttpContext, inputs: &[Option<&Payload>], _: FilterPhase) -> State {
        log::debug!("template: run - inputs: {:?}", inputs);

        let mut vs = Vec::new();
        let mut data = BTreeMap::new();

        for (input_name, input) in self.config.inputs.iter().zip(inputs.iter()) {
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
                            log::error!("template: input string is not valid UTF-8: {err}");
                        }
                    };
                }
                Some(Payload::Error(error)) => {
                    vs.push((input_name, serde_json::json!(error)));
                }
                None => {}
            }
        }

        for (input_name, v) in vs.iter() {
            data.insert(input_name, v);
        }

        match self.handlebars.render("template", &data) {
            Ok(output) => {
                log::debug!("output: {output}");
                match Payload::from_bytes(output.into(), Some(&self.config.content_type)) {
                    p @ Some(Payload::Error(_)) => State::Fail(p),
                    p => State::Done(p),
                }
            }
            Err(err) => State::Fail(Some(Payload::Error(format!(
                "error rendering template: {err}"
            )))),
        }
    }
}

pub struct TemplateFactory {}

impl NodeFactory for TemplateFactory {
    fn new_config(
        &self,
        _name: &str,
        inputs: &[String],
        bt: &BTreeMap<String, Value>,
    ) -> Result<Box<dyn NodeConfig>, String> {
        Ok(Box::new(TemplateConfig {
            inputs: inputs.to_vec(),
            template: get_config_value(bt, "template").unwrap_or_else(|| String::from("")),
            content_type: get_config_value(bt, "content_type")
                .unwrap_or_else(|| String::from("application/json")),
        }))
    }

    fn new_node(&self, config: &dyn NodeConfig) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<TemplateConfig>() {
            Some(cc) => Box::new(Template::new(cc.clone())),
            None => panic!("incompatible NodeConfig"),
        }
    }
}
