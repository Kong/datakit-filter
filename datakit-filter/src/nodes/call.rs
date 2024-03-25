use log;
use proxy_wasm::{traits::*, types::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::time::Duration;
use url::Url;

use crate::data::{Payload, State, State::*};
use crate::nodes::Connections;
use crate::nodes::{Node, NodeConfig};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CallConfig {
    connections: Connections,

    // FIXME: the optional ones should be Option,
    // but we're not really serializing this for now, just deserializing...

    // node-specific configuration fields:
    url: String,
    method: String,
    timeout: u32,
}

impl NodeConfig for CallConfig {
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
        "call"
    }

    fn from_map(bt: BTreeMap<String, Value>, connections: Connections) -> Box<dyn NodeConfig>
    where
        Self: Sized,
    {
        Box::new(CallConfig {
            connections,
            url: get_key(&bt, "url", String::from("")),
            method: get_key(&bt, "method", String::from("GET")),
            timeout: get_key(&bt, "timeout", 60),
        })
    }
}

#[derive(Clone)]
pub struct Call {
    config: CallConfig,

    token_id: Option<u32>,
}

impl Call {
    fn dispatch_call(
        &self,
        ctx: &dyn HttpContext,
        body: Option<&Payload>,
        headers: Option<&Payload>,
    ) -> Result<u32, Status> {
        log::info!(
            "call: {} - url: {}",
            self.config.connections.name,
            self.config.url
        );

        let call_url = Url::parse(self.config.url.as_str()).map_err(|r| {
            log::error!("call: failed parsing URL from 'url' field: {}", r);
            Status::BadArgument
        })?;

        let host = match call_url.host_str() {
            Some(h) => Ok(h),
            None => {
                log::error!("call: failed getting host from URL");
                Err(Status::BadArgument)
            }
        }?;

        let mut headers_vec: Vec<(&str, &str)> =
            headers.map_or_else(Vec::new, |p| p.to_headers_vec());
        headers_vec.push((":method", self.config.method.as_str()));
        headers_vec.push((":path", call_url.path()));

        let body_vec: Option<Vec<u8>> = body.map(|p| p.to_bytes());
        let body_slice: Option<Box<[u8]>> = body_vec.map(Vec::into_boxed_slice);

        let trailers = vec![];
        let timeout = Duration::from_secs(self.config.timeout.into());

        let sch_host_port = match call_url.port() {
            Some(port) => format!("{}:{}", host, port),
            None => host.to_owned(),
        };
        ctx.dispatch_http_call(
            &sch_host_port,
            headers_vec,
            body_slice.as_deref(),
            trailers,
            timeout,
        )
    }
}

fn get_key<T: for<'de> serde::Deserialize<'de>>(
    bt: &BTreeMap<String, Value>,
    key: &str,
    default: T,
) -> T {
    match bt.get(key) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(s) => s,
            Err(_) => default,
        },
        None => default,
    }
}

impl Node for Call {
    fn new(config: &Box<dyn NodeConfig>) -> Box<dyn Node> {
        match config.as_any().downcast_ref::<CallConfig>() {
            Some(cc) => Box::new(Call {
                config: cc.clone(),
                token_id: None,
            }),
            None => panic!("incompatible NodeConfig"),
        }
    }

    fn get_name(&self) -> &str {
        &self.config.connections.name
    }

    fn run(&mut self, ctx: &dyn HttpContext, inputs: Vec<&Payload>) -> State {
        log::info!("Call: on http request headers");

        match self.dispatch_call(ctx, inputs.first().copied(), inputs.get(1).copied()) {
            Ok(id) => {
                log::info!("call: dispatch call id: {:?}", id);
                self.token_id = Some(id);
                return Waiting(id);
            }
            Err(status) => {
                log::error!("call: error: {:?}", status);
            }
        }

        Done(None)
    }

    fn on_http_call_response(
        &mut self,
        ctx: &dyn HttpContext,
        _inputs: Vec<&Payload>,
        body_size: usize,
    ) -> State {
        log::info!("call: on http call response");

        let r = if let Some(body) = ctx.get_http_call_response_body(0, body_size) {
            Payload::from_bytes(body, ctx.get_http_call_response_header("Content-Type"))
        } else {
            None
        };

        Done(r)
    }

    fn is_waiting_on(&self, token_id: u32) -> bool {
        self.token_id == Some(token_id)
    }
}
