use jaq_core;
use jaq_interpret::{Ctx, Filter, FilterT, ParseCtx, RcIter, Val};
use jaq_std;
use proxy_wasm::traits::*;
use serde_json::Value as JsonValue;
use std::any::Any;
use std::collections::BTreeMap;
use std::str::from_utf8;

use crate::config::get_config_value;
use crate::data::{Payload, State};
use crate::nodes::{Node, NodeConfig, NodeFactory};

#[derive(Clone, Debug)]
pub struct JqConfig {
    filter: String,
    inputs: Vec<String>,
}

impl NodeConfig for JqConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
pub struct Jq {
    inputs: Vec<String>,
    filter: Filter,
}

impl TryFrom<&JqConfig> for Jq {
    type Error = String;

    fn try_from(config: &JqConfig) -> Result<Self, Self::Error> {
        Jq::new(&config.filter, config.inputs.clone())
    }
}

struct Errors(Vec<String>);

impl<T: Into<String>> From<T> for Errors {
    fn from(value: T) -> Self {
        Errors(vec![value.into()])
    }
}

impl Errors {
    fn new() -> Self {
        Self(vec![])
    }

    fn push<E>(&mut self, e: E)
    where
        E: Into<String>,
    {
        self.0.push(e.into());
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[cfg(test)]
    fn into_inner(self) -> Vec<String> {
        self.0
    }
}

impl From<Errors> for State {
    fn from(val: Errors) -> Self {
        State::Fail(Some(Payload::Error(if val.is_empty() {
            // should be unreachable
            "unknown jq error".to_string()
        } else {
            val.0.join(", ")
        })))
    }
}

impl Jq {
    fn new(filter: &str, inputs: Vec<String>) -> Result<Self, String> {
        let mut defs = ParseCtx::new(inputs.clone());

        defs.insert_natives(jaq_core::core());
        defs.insert_defs(jaq_std::std());

        if !defs.errs.is_empty() {
            for (err, _) in defs.errs {
                log::error!("filter input error: {err}");
            }
            return Err("failed parsing filter inputs".to_string());
        }

        let (parsed, errs) = jaq_parse::parse(filter, jaq_parse::main());
        if !errs.is_empty() {
            for err in errs {
                log::error!("filter parse error: {err}");
            }
            return Err("invalid filter".to_string());
        }

        let Some(parsed) = parsed else {
            return Err("parsed filter contains no main handler".to_string());
        };

        // compile the filter in the context of the given definitions
        let filter = defs.compile(parsed);
        if !defs.errs.is_empty() {
            for (err, _) in defs.errs {
                log::error!("filter compile error: {err}");
            }
            return Err("filter compilation failed".to_string());
        }

        let inputs = inputs.clone();

        Ok(Jq { inputs, filter })
    }

    fn exec(&self, inputs: &[Option<&Payload>]) -> Result<Vec<JsonValue>, Errors> {
        if inputs.len() != self.inputs.len() {
            return Err(Errors::from(format!(
                "invalid number of inputs, expected: {}, got: {}",
                self.inputs.len(),
                inputs.len()
            )));
        }

        let mut errs = Errors::new();

        let vars_iter = self
            .inputs
            .iter()
            .zip(inputs.iter())
            .map(|(name, input)| -> Val {
                match input {
                    Some(Payload::Json(value)) => value.clone().into(),
                    Some(Payload::Raw(bytes)) => match from_utf8(bytes) {
                        Ok(s) => Val::str(s.to_string()),
                        Err(e) => {
                            errs.push(format!("jq: input for {name} is not valid UTF-8: {e}"));
                            Val::Null
                        }
                    },
                    Some(Payload::Error(e)) => {
                        log::warn!("input error from previous node: {e}");
                        Val::Null
                    }
                    None => Val::Null,
                }
            });

        let input_iter = {
            let iter = std::iter::empty::<Result<Val, String>>();
            let iter = Box::new(iter) as Box<dyn Iterator<Item = Result<Val, String>>>;
            RcIter::new(iter)
        };
        let input = Val::Null;

        let ctx = Ctx::new(vars_iter, &input_iter);

        let results: Vec<JsonValue> = self
            .filter
            .run((ctx, input))
            .map(|item| match item {
                Ok(v) => v.into(),
                Err(e) => {
                    errs.push(e.to_string());
                    JsonValue::Null
                }
            })
            .collect();

        if !errs.is_empty() {
            return Err(errs);
        }

        Ok(results)
    }
}

impl Node for Jq {
    fn run(&self, _ctx: &dyn HttpContext, inputs: &[Option<&Payload>]) -> State {
        match self.exec(inputs) {
            Ok(mut results) => {
                State::Done(match results.len() {
                    // empty
                    0 => None,

                    // single
                    1 => {
                        let Some(item) = results.pop() else {
                            unreachable!();
                        };
                        Some(Payload::Json(item))
                    }

                    // more than one, return as an array
                    _ => Some(Payload::Json(results.into())),
                })
            }
            Err(errs) => errs.into(),
        }
    }
}

pub struct JqFactory {}

impl NodeFactory for JqFactory {
    fn new_config(
        &self,
        _name: &str,
        inputs: &[String],
        bt: &BTreeMap<String, JsonValue>,
    ) -> Result<Box<dyn NodeConfig>, String> {
        Ok(Box::new(JqConfig {
            filter: get_config_value(bt, "filter")
                .ok_or_else(|| "no filter specified".to_string())?,
            inputs: inputs.to_vec(),
        }))
    }

    fn new_node(&self, config: &dyn NodeConfig) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<JqConfig>() {
            Some(cc) => Box::new(Jq::try_from(cc).unwrap()),
            None => panic!("incompatible NodeConfig"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn filter_sanity() {
        let jq = Jq::new("{ a: $a, b: $b }", vec!["a".to_string(), "b".to_string()]);

        let Ok(jq) = jq else {
            panic!("jq error");
        };

        let a = Payload::Json(json!({
            "foo": "bar",
            "arr": [1, 2, 3],
        }));

        let b = Payload::Json(json!("some text"));

        let inputs = vec![Some(&a), Some(&b)];

        let res = jq.exec(inputs.as_slice());

        let Ok(results) = res else {
            panic!("unexpected jq error");
        };

        assert_eq!(
            results,
            vec![json!({
                "a": {
                    "foo": "bar",
                    "arr": [1, 2, 3]
                },
                "b": "some text"
            })]
        );
    }

    #[test]
    fn invalid_filter_text() {
        let jq = Jq::new("nope!", Vec::new());

        let Err(e) = jq else {
            panic!("expected invalid filter to result in an error");
        };

        assert_eq!("invalid filter", e.to_string());
    }

    #[test]
    fn empty_filter() {
        let jq = Jq::new("", vec![]);

        let Err(e) = jq else {
            panic!("expected invalid filter to result in an error");
        };

        assert_eq!("invalid filter", e.to_string());
    }

    #[test]
    fn filter_errors() {
        let jq = Jq::new("error(\"woops\")", vec![]).unwrap();

        let res = jq.exec(&[]);
        let Err(errs) = res else {
            panic!("expected a failure");
        };

        assert_eq!(errs.into_inner(), vec!["woops"]);
    }

    #[test]
    fn invalid_number_of_inputs() {
        let jq = Jq::new("$foo", vec!["foo".to_string()]).unwrap();

        let res = jq.exec(&[]);
        let Err(errs) = res else {
            panic!("expected a failure");
        };

        assert_eq!(
            errs.into_inner(),
            vec!["invalid number of inputs, expected: 1, got: 0"]
        );
    }
}
