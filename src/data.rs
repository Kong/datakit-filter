use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::dependency_graph::DependencyGraph;

#[allow(clippy::enum_variant_names)]
#[derive(PartialEq, Clone, Copy)]
pub enum Phase {
    HttpRequestHeaders,
    HttpRequestBody,
    HttpResponseHeaders,
    HttpResponseBody,
    HttpCallResponse,
}

pub struct Input<'a> {
    pub data: &'a [Option<&'a Payload>],
    pub phase: Phase,
}

#[derive(Debug)]
pub enum Payload {
    Raw(Vec<u8>),
    Json(serde_json::Value),
    Error(String),
}

impl Payload {
    pub fn content_type(&self) -> Option<&str> {
        match &self {
            Payload::Json(_) => Some("application/json"),
            _ => None,
        }
    }

    pub fn from_bytes(bytes: Vec<u8>, content_type: Option<&str>) -> Option<Payload> {
        match content_type {
            Some(ct) => {
                if ct == "application/json" {
                    match serde_json::from_slice(&bytes) {
                        Ok(v) => Some(Payload::Json(v)),
                        Err(e) => Some(Payload::Error(e.to_string())),
                    }
                } else {
                    Some(Payload::Raw(bytes))
                }
            }
            _ => None,
        }
    }

    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        match &self {
            Payload::Json(value) => Ok(value.clone()),
            Payload::Raw(vec) => match std::str::from_utf8(vec) {
                Ok(s) => serde_json::to_value(s).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            },
            Payload::Error(e) => Err(e.clone()),
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        match &self {
            Payload::Json(value) => match serde_json::to_string(value) {
                Ok(s) => Ok(s.into_bytes()),
                Err(e) => Err(e.to_string()),
            },
            Payload::Raw(s) => Ok(s.clone()), // it would be nice to be able to avoid this copy
            Payload::Error(e) => Err(e.clone()),
        }
    }

    pub fn len(&self) -> Option<usize> {
        match &self {
            Payload::Json(_) => None,
            Payload::Raw(s) => Some(s.len()),
            Payload::Error(e) => Some(e.len()),
        }
    }

    pub fn to_pwm_headers(&self) -> Vec<(&str, &str)> {
        match &self {
            Payload::Json(value) => {
                let mut vec: Vec<(&str, &str)> = vec![];
                if let serde_json::Value::Object(map) = value {
                    for (k, entry) in map {
                        match entry {
                            serde_json::Value::Array(vs) => {
                                for v in vs {
                                    if let serde_json::Value::String(s) = v {
                                        vec.push((k, s));
                                    }
                                }
                            }

                            // accept string values as well
                            serde_json::Value::String(s) => {
                                vec.push((k, s));
                            }

                            _ => {}
                        }
                    }
                }

                vec
            }
            _ => {
                // TODO
                log::debug!("NYI: converting payload into headers vector");
                vec![]
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}

pub fn from_pwm_headers(vec: Vec<(String, String)>) -> Payload {
    let mut map = BTreeMap::new();
    for (k, v) in vec {
        let lk = k.to_lowercase();
        if let Some(vs) = map.get_mut(&lk) {
            match vs {
                StringOrVec::String(s) => {
                    let ss = s.to_string();
                    map.insert(lk, StringOrVec::Vec(vec![ss, v]));
                }
                StringOrVec::Vec(vs) => {
                    vs.push(v);
                }
            };
        } else {
            map.insert(lk, StringOrVec::String(v));
        }
    }

    let value = serde_json::to_value(map).expect("serializable map");
    Payload::Json(value)
}

pub fn to_pwm_headers(payload: Option<&Payload>) -> Vec<(&str, &str)> {
    payload.map_or_else(Vec::new, |p| p.to_pwm_headers())
}

/// To use this result in proxy-wasm calls as an Option<&[u8]>, use:
/// `data::to_pwm_body(p).as_deref()`.
pub fn to_pwm_body(payload: Option<&Payload>) -> Result<Option<Box<[u8]>>, String> {
    match payload {
        Some(p) => match p.to_bytes() {
            Ok(b) => Ok(Some(Vec::into_boxed_slice(b))),
            Err(e) => Err(e),
        },
        None => Ok(None),
    }
}

#[derive(Debug)]
pub enum State {
    Waiting(u32),
    Done(Option<Payload>),
    Fail(Option<Payload>),
}

#[derive(Default)]
pub struct Data {
    graph: DependencyGraph,
    states: BTreeMap<String, State>,
}

impl Data {
    pub fn new(graph: DependencyGraph) -> Data {
        Data {
            graph,
            states: Default::default(),
        }
    }

    pub fn set(&mut self, name: &str, state: State) {
        self.states.insert(name.to_string(), state);
    }

    fn can_trigger(&self, name: &str, waiting: Option<u32>) -> bool {
        // If node is Done, avoid producing inputs
        // and re-triggering its execution.
        if let Some(state) = self.states.get(name) {
            match state {
                State::Done(_) => {
                    return false;
                }
                State::Waiting(w) => match &waiting {
                    Some(id) => {
                        if w != id {
                            return false;
                        }
                    }
                    None => return false,
                },
                State::Fail(_) => {
                    return false;
                }
            }
        }

        // Check that all inputs have payloads available
        for input in self.graph.each_input(name) {
            let val = self.states.get(input);
            match val {
                Some(State::Done(_)) => {}
                _ => {
                    return false;
                }
            };
        }

        true
    }

    pub fn get_inputs_for(
        &self,
        name: &str,
        waiting: Option<u32>,
    ) -> Option<Vec<Option<&Payload>>> {
        if !self.can_trigger(name, waiting) {
            return None;
        }

        // If so, allocate the vector with the result.
        let mut vec: Vec<Option<&Payload>> = Vec::new();
        for input in self.graph.each_input(name) {
            if let Some(State::Done(p)) = self.states.get(input) {
                vec.push(p.as_ref());
            }
        }

        Some(vec)
    }

    /// If the node is triggerable, that is, it has all its required
    /// inputs available to trigger (i.e. none of its inputs are in a
    /// `Waiting` state), then return the payload of the first input that
    /// is in a `Done state.
    ///
    /// Note that by returning an `Option<&Payload>` this makes no
    /// distinction between the node being not triggerable or the
    /// node being triggerable via a `Done(None)` input.
    ///
    /// This is not an issue because this function is intended for use
    /// with the implicit nodes (`response_body`, etc.) which are
    /// handled as special cases directly by the filter.
    pub fn first_input_for(&self, name: &str, waiting: Option<u32>) -> Option<&Payload> {
        if !self.can_trigger(name, waiting) {
            return None;
        }

        for input in self.graph.each_input(name) {
            if let Some(State::Done(p)) = self.states.get(input) {
                return p.as_ref();
            }
        }

        None
    }
}
