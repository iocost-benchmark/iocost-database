// Copyright (c) Facebook, Inc. and its affiliates.
use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};
use systemd::UnitState as US;

use rd_agent_intf::{RunnerState, Slice, HASHD_BENCH_SVC_NAME, IOCOST_BENCH_SVC_NAME};
use rd_util::*;

use super::hashd::HashdSet;
use super::side::{Balloon, SideRunner, Sideload, Sysload};
use super::{bench, report, slices};
use super::{Config, SysObjs};

const HEALTH_CHECK_INTV: Duration = Duration::from_secs(10);

use RunnerState::*;

pub struct RunnerData {
    pub cfg: Arc<Config>,
    pub sobjs: SysObjs,
    pub state: RunnerState,
    warned_bench: bool,
    warned_init: bool,
    force_apply: bool,

    pub bench_hashd: Option<TransientService>,
    pub bench_iocost: Option<TransientService>,

    pub hashd_set: HashdSet,
    pub side_runner: SideRunner,
    pub balloon: Balloon,
}

impl RunnerData {
    fn new(cfg: Config, sobjs: SysObjs) -> Self {
        let cfg = Arc::new(cfg);
        Self {
            sobjs,
            state: Idle,
            warned_bench: false,
            warned_init: false,
            force_apply: false,
            bench_hashd: None,
            bench_iocost: None,
            hashd_set: HashdSet::new(&cfg),
            side_runner: SideRunner::new(cfg.clone()),
            balloon: Balloon::new(cfg.clone()),
            cfg,
        }
    }

    pub fn all_svcs(&self) -> HashSet<(String, String)> {
        let mut svcs = HashSet::<(String, String)>::new();
        if self.bench_hashd.is_some() {
            svcs.insert((
                HASHD_BENCH_SVC_NAME.to_owned(),
                format!("{}/{}", Slice::Work.cgrp(), HASHD_BENCH_SVC_NAME),
            ));
        }
        if self.bench_iocost.is_some() {
            svcs.insert((
                IOCOST_BENCH_SVC_NAME.to_owned(),
                format!("{}/{}", Slice::Work.cgrp(), HASHD_BENCH_SVC_NAME),
            ));
        }
        for svc in self
            .hashd_set
            .all_svcs()
            .drain()
            .chain(self.side_runner.all_svcs().drain())
        {
            svcs.insert(svc);
        }
        svcs
    }

    fn become_idle(&mut self) {
        info!("cmd: Transitioning to Idle state");
        self.bench_hashd = None;
        self.bench_iocost = None;
        self.hashd_set.stop();
        self.side_runner.stop();
        self.state = Idle;
    }

    fn maybe_reload_one<T: JsonLoad + JsonSave>(cfile: &mut JsonConfigFile<T>) -> bool {
        match cfile.maybe_reload() {
            Ok(true) => {
                debug!("cmd: Reloaded {:?}", &cfile.path.as_ref().unwrap());
                true
            }
            Ok(false) => false,
            Err(e) => {
                warn!("cmd: Failed to reload {:?} ({:?})", cfile.path, &e);
                false
            }
        }
    }

    fn maybe_reload(&mut self) -> bool {
        let sobjs = &mut self.sobjs;
        let last_cpu_headroom = sobjs.cmd_file.data.sideloader.cpu_headroom;

        // Configs are controlled by benchmarks while they're running, don't
        // reload.
        let (re_bench, re_slice, _re_side, re_oomd) = match self.state {
            BenchIoCost | BenchHashd => (false, false, false, false),
            _ => {
                let force = self.force_apply;
                self.force_apply = false;
                (
                    Self::maybe_reload_one(&mut sobjs.bench_file) || force,
                    Self::maybe_reload_one(&mut sobjs.slice_file) || force,
                    Self::maybe_reload_one(&mut sobjs.side_def_file) || force,
                    Self::maybe_reload_one(&mut sobjs.oomd.file) || force,
                )
            }
        };
        let re_cmd = Self::maybe_reload_one(&mut sobjs.cmd_file);

        let mem_size = sobjs.bench_file.data.hashd.actual_mem_size();

        if re_bench {
            if let Err(e) = bench::apply_iocost(&mut sobjs.bench_file.data, &self.cfg) {
                warn!(
                    "cmd: Failed to apply changed iocost configuration on {:?} ({:?})",
                    self.cfg.scr_dev, &e
                );
            }
        }

        if re_bench || re_slice {
            if let Err(e) = slices::apply_slices(&mut sobjs.slice_file.data, mem_size, &self.cfg) {
                warn!("cmd: Failed to apply updated slice overrides ({:?})", &e);
            }
        }

        if (re_bench || re_oomd) && self.cfg.enforce.oomd {
            if let Err(e) = sobjs.oomd.apply() {
                error!("cmd: Failed to apply oomd configuration ({:?})", &e);
                panic!();
            }
        }

        let mut apply_sideloader = false;

        if re_slice {
            if sobjs
                .slice_file
                .data
                .controlls_disabled(super::instance_seq())
            {
                if sobjs.sideloader.svc.unit.state == US::Running {
                    info!("cmd: Controllers are being forced off, disabling sideloader");
                    let _ = sobjs.sideloader.svc.unit.stop();
                }
            } else {
                if sobjs.sideloader.svc.unit.state != US::Running {
                    info!("cmd: All controller enabled, enabling sideloader");
                    apply_sideloader = true;
                }
            }
        }

        if sobjs.cmd_file.data.sideloader.cpu_headroom != last_cpu_headroom {
            info!(
                "cmd: Updating sideloader headroom from {:.2} to {:.2}",
                last_cpu_headroom, sobjs.cmd_file.data.sideloader.cpu_headroom
            );
            apply_sideloader = true;
        }

        if apply_sideloader && self.cfg.enforce.all() {
            let sideloader_cmd = &sobjs.cmd_file.data.sideloader;
            let slice_knobs = &sobjs.slice_file.data;
            if let Err(e) = sobjs.sideloader.apply(sideloader_cmd, slice_knobs) {
                error!("cmd: Failed to apply sideloader changes ({:?})", &e);
                panic!();
            }
        }

        re_bench || re_cmd || re_slice
    }

    fn apply_swappiness(&self, swappiness: Option<u32>) -> Result<()> {
        if !self.cfg.enforce.mem {
            return Ok(());
        }
        let cur = read_swappiness()?;
        let target = swappiness
            .unwrap_or(self.cfg.sr_swappiness.unwrap())
            .min(200);
        if cur != target {
            if target >= 60 {
                info!("cmd: Updating swappiness {} -> {}", cur, target);
            } else {
                warn!("cmd: Updating swappiness {} -> {} (< 60)", cur, target);
            }
            write_one_line(SWAPPINESS_PATH, &format!("{}", target))
                .context("Updating swappiness")?;
        }
        Ok(())
    }

    fn apply_zswap_enabled(&self, enabled: Option<bool>) -> Result<()> {
        if !self.cfg.enforce.mem {
            return Ok(());
        }
        let cur = read_zswap_enabled()?;
        let target = enabled.unwrap_or(self.cfg.sr_zswap_enabled.unwrap());
        if cur != target {
            write_one_line(ZSWAP_ENABLED_PATH, if target { "Y" } else { "N" })
                .context("Updating zswap enable")?;
        }
        Ok(())
    }

    fn apply_workloads(&mut self) -> Result<()> {
        let cmd = &self.sobjs.cmd_file.data;
        let bench = &self.sobjs.bench_file.data;
        let mem_low = self.sobjs.slice_file.data[Slice::Work]
            .mem_low
            .nr_bytes(false);

        self.hashd_set.apply(&cmd.hashd, &bench.hashd, mem_low)?;
        Ok(())
    }

    fn apply_cmd(
        &mut self,
        removed_sysloads: &mut Vec<Sysload>,
        removed_sideloads: &mut Vec<Sideload>,
    ) -> Result<bool> {
        let cmd = &self.sobjs.cmd_file.data;
        let bench = &self.sobjs.bench_file.data;
        let mut repeat = false;

        self.sobjs.cmd_ack_file.data.cmd_seq = cmd.cmd_seq;
        if let Err(e) = self.sobjs.cmd_ack_file.commit() {
            warn!(
                "cmd: Failed to update {:?} ({:?})",
                &self.cfg.cmd_ack_path, &e
            );
        }

        self.apply_swappiness(cmd.swappiness)?;
        self.apply_zswap_enabled(cmd.zswap_enabled)?;

        match self.state {
            Idle => {
                if cmd.bench_iocost_seq > bench.iocost_seq {
                    self.bench_iocost = Some(bench::start_iocost_bench(&*self.cfg)?);
                    self.state = BenchIoCost;
                    self.force_apply = true;
                } else if cmd.bench_hashd_seq > bench.hashd_seq {
                    if bench.iocost_seq > 0 || self.cfg.force_running {
                        if let Err(e) = self.balloon.set_size(cmd.bench_hashd_balloon_size) {
                            error!(
                                "cmd: Failed to set balloon size to {:.2}G for hashd bench ({:?})",
                                to_gb(cmd.bench_hashd_balloon_size),
                                &e
                            );
                            panic!();
                        }

                        self.sobjs.oomd.stop();

                        self.bench_hashd = Some(bench::start_hashd_bench(
                            &*self.cfg,
                            cmd.hashd[0].log_bps,
                            0,
                            cmd.bench_hashd_args.clone(),
                        )?);
                        self.hashd_set.mark_bench_start();

                        self.state = BenchHashd;
                        self.force_apply = true;
                    } else if !self.warned_bench {
                        warn!("cmd: iocost benchmark must be run before hashd benchmark");
                        self.warned_bench = true;
                    }
                } else if bench.hashd_seq > 0 || self.cfg.force_running {
                    info!("cmd: Transitioning to Running state");
                    self.state = Running;
                    repeat = true;
                } else if !self.warned_init {
                    warn!("cmd: hashd benchmark hasn't been run yet, staying idle");
                    self.warned_init = true;
                }
            }
            Running => {
                if cmd.bench_hashd_seq > bench.hashd_seq || cmd.bench_iocost_seq > bench.iocost_seq
                {
                    self.become_idle();
                } else {
                    if let Err(e) = self.apply_workloads() {
                        error!("cmd: Failed to apply workload changes ({:?})", &e);
                        panic!();
                    }

                    let side_defs = &self.sobjs.side_def_file.data;
                    let sysload_target = &self.sobjs.cmd_file.data.sysloads;
                    if let Err(e) = self.side_runner.apply_sysloads(
                        sysload_target,
                        side_defs,
                        &self.sobjs.bench_file.data,
                        Some(removed_sysloads),
                    ) {
                        warn!("cmd: Failed to apply sysload changes ({:?})", &e);
                    }
                    let sideload_target = &self.sobjs.cmd_file.data.sideloads;
                    if let Err(e) = self.side_runner.apply_sideloads(
                        sideload_target,
                        side_defs,
                        &self.sobjs.bench_file.data,
                        Some(removed_sideloads),
                    ) {
                        warn!("cmd: Failed to apply sideload changes ({:?})", &e);
                    }

                    let balloon_size = ((total_memory() as f64)
                        * &self.sobjs.cmd_file.data.balloon_ratio)
                        as usize;
                    if let Err(e) = self.balloon.set_size(balloon_size) {
                        error!(
                            "cmd: Failed to set balloon size to {:.2}G ({:?})",
                            to_gb(balloon_size),
                            &e
                        );
                        panic!();
                    }
                }
            }
            BenchHashd => {
                if cmd.bench_hashd_seq <= bench.hashd_seq {
                    info!("cmd: Canceling hashd benchmark");
                    self.become_idle();
                }
            }
            BenchIoCost => {
                if cmd.bench_iocost_seq <= bench.iocost_seq {
                    info!("cmd: Canceling iocost benchmark");
                    self.become_idle();
                }
            }
        }
        if self.state != Idle {
            self.warned_bench = false;
            self.warned_init = false;
        }
        Ok(repeat)
    }

    fn check_completions(&mut self) -> Result<()> {
        match self.state {
            BenchHashd | BenchIoCost => {
                let svc = if self.state == BenchHashd {
                    self.bench_hashd.as_mut().unwrap()
                } else {
                    self.bench_iocost.as_mut().unwrap()
                };
                svc.unit.refresh()?;
                match &svc.unit.state {
                    US::Running => Ok(()),
                    US::Exited => {
                        info!("cmd: benchmark finished, loading the results");
                        let cmd = &mut self.sobjs.cmd_file.data;
                        let bf = &mut self.sobjs.bench_file;
                        if self.state == BenchHashd {
                            bench::update_hashd(&mut bf.data, &self.cfg, cmd.bench_hashd_seq)?;
                            bf.save()?;
                        } else {
                            bench::update_iocost(&mut bf.data, &self.cfg, cmd.bench_iocost_seq)?;
                            bf.save()?;
                            bench::apply_iocost(&bf.data, &self.cfg)?;
                        }
                        self.become_idle();
                        Ok(())
                    }
                    state => {
                        warn!("cmd: Invalid state {:?} for {}", &state, &svc.unit.name);
                        self.become_idle();
                        Ok(())
                    }
                }
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone)]
pub struct Runner {
    pub data: Arc<Mutex<RunnerData>>,
}

impl Runner {
    pub fn new(cfg: Config, sobjs: SysObjs) -> Self {
        Self {
            data: Arc::new(Mutex::new(RunnerData::new(cfg, sobjs))),
        }
    }

    pub fn run(&mut self) {
        let mut reporter = None;
        let mut last_health_check_at = Instant::now();
        let mut cmd_pending = true;
        let mut verify_pending = false;

        let mut data = self.data.lock().unwrap();

        while !prog_exiting() {
            // apply commands and check for completions
            let mut removed_sysloads = Vec::new();
            let mut removed_sideloads = Vec::new();

            if cmd_pending || data.state == Idle {
                cmd_pending = false;
                loop {
                    match data.apply_cmd(&mut removed_sysloads, &mut removed_sideloads) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(e) => {
                            warn!("cmd: Failed to apply commands ({:?})", &e);
                            break;
                        }
                    }
                }
            }

            if let Err(e) = data.check_completions() {
                warn!("cmd: Failed to check completions ({:?})", &e);
            }

            // Stopping sys/sideloads and clearing scratch dirs can
            // take a while. Do it unlocked so that it doesn't stall
            // reports.
            drop(data);
            drop(removed_sysloads);
            drop(removed_sideloads);

            if reporter.is_none() {
                reporter = Some(match report::Reporter::new(self.clone()) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("cmd: Failed to start reporter ({:?})", &e);
                        panic!();
                    }
                });
            }

            // sleep a bit and start the next iteration
            sleep(Duration::from_millis(100));

            data = self.data.lock().unwrap();
            let now = Instant::now();

            if !data.cfg.bypass
                && (now.duration_since(last_health_check_at) >= HEALTH_CHECK_INTV || verify_pending)
            {
                let workload_senpai = data.sobjs.oomd.workload_senpai_enabled();
                if let Err(e) = slices::verify_and_fix_slices(
                    &data.sobjs.slice_file.data,
                    workload_senpai,
                    &data.cfg,
                ) {
                    warn!("cmd: Health check failed ({:?})", &e);
                }

                if data.cfg.enforce.io {
                    if let Err(e) = super::set_iosched(&data.cfg.scr_dev, "none") {
                        error!(
                            "cfg: Failed to set none iosched on {:?} ({})",
                            &data.cfg.scr_dev, &e
                        );
                    }
                }

                last_health_check_at = now;
                verify_pending = false;
            }

            if data.maybe_reload() {
                cmd_pending = true;
                verify_pending = true;
            }
        }
    }
}
