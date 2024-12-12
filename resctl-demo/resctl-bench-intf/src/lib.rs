// Copyright (c) Facebook, Inc. and its affiliates.
use rd_util::*;

pub mod args;
pub mod iocost;
pub mod jobspec;

pub use args::{set_bench_list, Args, Mode, GITHUB_DOC_LINK};
pub use iocost::IoCostQoSOvr;
pub use jobspec::{format_job_props, JobProps, JobSpec};

lazy_static::lazy_static! {
    pub static ref VERSION: &'static str = env!("CARGO_PKG_VERSION");
    pub static ref FULL_VERSION: String = full_version(*VERSION);
}
