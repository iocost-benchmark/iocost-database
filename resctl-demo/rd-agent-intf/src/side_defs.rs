// Copyright (c) Facebook, Inc. and its affiliates.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use rd_util::*;

const SIDE_DEF_DOC: &str = "\
//
// rd-agent side/sysload definitions
//
//  DEF_ID.args[]: Command arguments
//  DEF_ID.frozen_exp: Sideloader frozen expiration duration
//
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideloadSpec {
    pub args: Vec<String>,
    pub frozen_exp: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct SideloadDefs {
    #[serde(flatten)]
    pub defs: BTreeMap<String, SideloadSpec>,
}

impl Default for SideloadDefs {
    fn default() -> Self {
        Self {
            defs: [
                (
                    "build-linux-half".into(),
                    SideloadSpec {
                        args: vec![
                            "build-linux.sh".into(),
                            "allmodconfig".into(),
                            "1".into(),
                            "2".into(),
                        ],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-1x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into(), "1".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-2x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into(), "2".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-4x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into(), "4".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-8x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into(), "8".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-16x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into(), "16".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-32x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into(), "32".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-unlimited".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allmodconfig".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-allnoconfig-1x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "allnoconfig".into(), "1".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "build-linux-defconfig-1x".into(),
                    SideloadSpec {
                        args: vec!["build-linux.sh".into(), "defconfig".into(), "1".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "mem-hog-10pct".into(),
                    SideloadSpec {
                        args: vec!["mem-hog.sh".into(), "10%".into(), "0%".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "mem-hog-25pct".into(),
                    SideloadSpec {
                        args: vec!["mem-hog.sh".into(), "25%".into(), "0%".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "mem-hog-50pct".into(),
                    SideloadSpec {
                        args: vec!["mem-hog.sh".into(), "50%".into(), "0%".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "mem-hog-1x".into(),
                    SideloadSpec {
                        args: vec!["mem-hog.sh".into(), "100%".into(), "0%".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "mem-hog-2x".into(),
                    SideloadSpec {
                        args: vec!["mem-hog.sh".into(), "200%".into(), "0%".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "mem-bloat-1x".into(),
                    SideloadSpec {
                        args: vec!["mem-hog.sh".into(), "1000%".into(), "100%".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "read-bomb".into(),
                    SideloadSpec {
                        args: vec!["read-bomb.py".into(), "1024".into(), "16384".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "burn-cpus-50pct".into(),
                    SideloadSpec {
                        args: vec!["burn-cpus.sh".into(), "1".into(), "2".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "burn-cpus-1x".into(),
                    SideloadSpec {
                        args: vec!["burn-cpus.sh".into(), "1".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "burn-cpus-2x".into(),
                    SideloadSpec {
                        args: vec!["burn-cpus.sh".into(), "2".into()],
                        frozen_exp: 30,
                    },
                ),
                (
                    "inodesteal-test".into(),
                    SideloadSpec {
                        args: vec!["inodesteal-test.py".into()],
                        frozen_exp: 30,
                    },
                ),
            ]
            .iter()
            .cloned()
            .collect(),
        }
    }
}

impl JsonLoad for SideloadDefs {}

impl JsonSave for SideloadDefs {
    fn preamble() -> Option<String> {
        Some(SIDE_DEF_DOC.to_string())
    }
}
