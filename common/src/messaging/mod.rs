use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use lapin::{
    BasicProperties, Channel, options::BasicPublishOptions, publisher_confirm::PublisherConfirm,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

include!("rabbitmq.rs");
include!("node_recon.rs");
include!("commands.rs");
include!("semantic_chain.rs");
include!("client_chat.rs");
include!("semantic_parser.rs");
include!("traffic_node.rs");
include!("system_state.rs");
