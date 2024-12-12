// Copyright (c) Facebook, Inc. and its affiliates.
use super::*;
use rand::Rng;

use super::protection::mem_hog_tune::{DFL_ISOL_PCT, DFL_ISOL_THR};
use super::protection::{self, ProtectionJob, ProtectionRecord, ProtectionResult};
use super::storage::{StorageJob, StorageRecord, StorageResult};
use rd_agent_intf::BenchKnobs;
use std::collections::BTreeMap;

// Gonna run storage bench multiple times with different parameters. Let's
// run it just once by default.
const DFL_VRATE_MAX: f64 = 100.0;
const DFL_VRATE_INTVS: u32 = 5;
const DFL_STOR_BASE_LOOPS: u32 = 3;
const DFL_STOR_LOOPS: u32 = 1;
const DFL_RETRIES: u32 = 1;

// Don't go below 1% of the specified model when applying vrate-intvs.
const VRATE_INTVS_MIN: f64 = 1.0;

#[derive(Default)]
pub struct IoCostQoSJob {
    stor_base_loops: u32,
    stor_loops: u32,
    isol_pct: String,
    isol_thr: f64,
    dither_dist: Option<f64>,
    ign_min_perf: bool,
    retries: u32,
    allow_fail: bool,
    stor_job: StorageJob,
    prot_job: ProtectionJob,
    runs: Vec<IoCostQoSOvr>,
}

pub struct IoCostQoSBench {}

impl Bench for IoCostQoSBench {
    fn desc(&self) -> BenchDesc {
        BenchDesc::new(
            "iocost-qos",
            "Benchmark IO isolation with different io.cost QoS configurations",
        )
        .takes_run_propsets()
        .takes_format_props()
        .incremental()
    }

    fn parse(&self, spec: &JobSpec, prev_data: Option<&JobData>) -> Result<Box<dyn Job>> {
        Ok(Box::new(IoCostQoSJob::parse(spec, prev_data)?))
    }

    fn doc<'a>(&self, out: &mut Box<dyn Write + 'a>) -> Result<()> {
        const DOC: &[u8] = include_bytes!("../../doc/iocost-qos.md");
        write!(out, "{}", String::from_utf8_lossy(DOC))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IoCostQoSRecordRun {
    pub period: (u64, u64),
    pub ovr: IoCostQoSOvr,
    pub qos: Option<IoCostQoSParams>,
    pub stor: StorageRecord,
    pub prot: ProtectionRecord,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct IoCostQoSRecord {
    pub base_model: IoCostModelParams,
    pub base_qos: IoCostQoSParams,
    pub mem_profile: u32,
    pub runs: Vec<Option<IoCostQoSRecordRun>>,
    dither_dist: Option<f64>,
    inc_runs: Vec<IoCostQoSRecordRun>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct IoCostQoSResultRun {
    pub stor: StorageResult,
    pub prot: ProtectionResult,
    pub adjusted_mem_size: Option<usize>,
    pub adjusted_mem_offload_factor: Option<f64>,
    pub adjusted_mem_offload_delta: Option<f64>,
    pub vrate: BTreeMap<String, f64>,
    pub iolat: [BTreeMap<String, BTreeMap<String, f64>>; 2],
    pub nr_reports: (u64, u64),
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct IoCostQoSResult {
    pub runs: Vec<Option<IoCostQoSResultRun>>,
}

impl IoCostQoSJob {
    const VRATE_PCTS: &'static [&'static str] = &[
        "00", "01", "10", "25", "50", "75", "90", "99", "100", "mean", "stdev",
    ];

    fn parse(spec: &JobSpec, prev_data: Option<&JobData>) -> Result<Self> {
        let mut stor_spec = JobSpec::new(
            "storage",
            None,
            Some(format!("none,{}", spec.passive.as_deref().unwrap_or("")).trim_end_matches(',')),
            JobSpec::props(&[&[("apply", "")]]),
        );
        let mut prot_spec = JobSpec::new(
            "protection",
            None,
            spec.passive.as_deref(),
            JobSpec::props(&[
                &[],
                &[
                    ("scenario", "mem-hog-tune"),
                    ("load", "1.0"),
                    ("size-min", "1"),
                    ("size-max", "1"),
                ],
            ]),
        );

        let mut vrate_min = 0.0;
        let mut vrate_max = DFL_VRATE_MAX;
        let mut vrate_intvs = 0;
        let mut stor_base_loops = DFL_STOR_BASE_LOOPS;
        let mut stor_loops = DFL_STOR_LOOPS;
        let mut isol_pct = DFL_ISOL_PCT.to_owned();
        let mut isol_thr = DFL_ISOL_THR;
        let mut retries = DFL_RETRIES;
        let mut allow_fail = false;
        let mut runs = vec![IoCostQoSOvr {
            off: true,
            ..Default::default()
        }];
        let mut dither = false;
        let mut dither_dist = None;
        let mut ign_min_perf = false;

        for (k, v) in spec.props[0].iter() {
            match k.as_str() {
                "vrate-min" => vrate_min = v.parse::<f64>()?,
                "vrate-max" => vrate_max = v.parse::<f64>()?,
                "vrate-intvs" => vrate_intvs = v.parse::<u32>()?,
                "dither" => {
                    dither = true;
                    if v.len() > 0 {
                        dither_dist = Some(v.parse::<f64>()?);
                    }
                }
                "storage-base-loops" => stor_base_loops = v.parse::<u32>()?,
                "storage-loops" => stor_loops = v.parse::<u32>()?,
                "isol-pct" => isol_pct = v.to_owned(),
                "isol-thr" => isol_thr = parse_frac(v)?,
                "retries" => retries = v.parse::<u32>()?,
                "allow-fail" => allow_fail = v.parse::<bool>()?,
                "ignore-min-perf" => ign_min_perf = v.len() == 0 || v.parse::<bool>()?,
                k if k.starts_with("storage-") => {
                    stor_spec.props[0].insert(k[8..].into(), v.into());
                }
                k => bail!("unknown property key {:?}", k),
            }
        }

        if vrate_min < 0.0 || vrate_max < 0.0 || vrate_min >= vrate_max {
            bail!("invalid vrate range [{}, {}]", vrate_min, vrate_max);
        }

        for props in spec.props[1..].iter() {
            let mut ovr = IoCostQoSOvr::default();
            for (k, v) in props.iter() {
                if !ovr.parse(k, v)? {
                    bail!("unknown property key {:?}", k);
                }
            }
            runs.push(ovr);
        }

        prot_spec.props[1].insert("isol-pct".to_owned(), isol_pct.clone());
        prot_spec.props[1].insert("isol-thr".to_owned(), format!("{}", isol_thr));

        let stor_job = StorageJob::parse(&stor_spec)?;
        let prot_job = ProtectionJob::parse(&prot_spec)?;

        if runs.len() == 1 && vrate_intvs == 0 {
            vrate_intvs = DFL_VRATE_INTVS;
        }

        if vrate_intvs > 0 {
            // min of 0 is special case and means that we start at one
            // click, so if min is 0, max is 10 and intvs is 5, the sequence
            // is (10, 7.5, 5, 2.5). If min > 0, the range is inclusive -
            // min 5, max 10, intvs 5 => (10, 9, 8, 7, 6, 5).
            let click;
            let mut dither_shift = 0.0;
            if vrate_min == 0.0 {
                click = vrate_max / vrate_intvs as f64;
                vrate_min = click;
                dither_shift = -click / 2.0;
            } else {
                click = (vrate_max - vrate_min) / (vrate_intvs - 1) as f64;
            };

            if dither {
                if dither_dist.is_none() {
                    if let Some(pd) = prev_data.as_ref() {
                        if pd.record.is_some() {
                            // If prev has dither_dist set, use the prev dither_dist
                            // so that we can use results from it.
                            let prec: IoCostQoSRecord = pd.parse_record()?;
                            if let Some(pdd) = prec.dither_dist.as_ref() {
                                dither_dist = Some(*pdd);
                            }
                        }
                    }
                }
                if dither_dist.is_none() {
                    dither_dist = Some(
                        rand::thread_rng().gen_range(-click / 2.0..click / 2.0) + dither_shift,
                    );
                }
                vrate_min += dither_dist.as_ref().unwrap();
                vrate_max += dither_dist.as_ref().unwrap();
            }

            vrate_min = vrate_min.max(VRATE_INTVS_MIN);

            let mut vrate = vrate_max;
            while vrate > vrate_min - 0.001 {
                let mut ovr = IoCostQoSOvr {
                    min: Some(vrate),
                    max: Some(vrate),
                    ..Default::default()
                };
                ovr.sanitize();
                runs.push(ovr);
                vrate -= click;
            }
        }

        Ok(IoCostQoSJob {
            stor_base_loops,
            stor_loops,
            isol_pct,
            isol_thr,
            dither_dist,
            ign_min_perf,
            retries,
            allow_fail,
            stor_job,
            prot_job,
            runs,
        })
    }

    fn prev_matches(&self, prec: &IoCostQoSRecord, mem_profile: u32, bench: &BenchKnobs) -> bool {
        // If @pr has't completed and only contains incremental results, its
        // mem_profile isn't initialized yet. Obtain mem_profile from the
        // base storage result instead.
        let base_rec = if prec.runs.len() > 0 && prec.runs[0].is_some() {
            prec.runs[0].as_ref().unwrap()
        } else if prec.inc_runs.len() > 0 {
            &prec.inc_runs[0]
        } else {
            return false;
        };

        let msg = "iocost-qos: Existing result doesn't match the current configuration";
        if prec.base_model != bench.iocost.model || prec.base_qos != bench.iocost.qos {
            warn!("{} (iocost parameter mismatch)", &msg);
            return false;
        }
        if mem_profile != base_rec.stor.mem.profile {
            warn!(
                "{} (mem-profile mismatch, {} != {})",
                &msg, mem_profile, base_rec.stor.mem.profile
            );
            return false;
        }

        true
    }

    fn find_matching_rec_run<'a>(
        ovr: &IoCostQoSOvr,
        prev_rec: &'a IoCostQoSRecord,
    ) -> Option<&'a IoCostQoSRecordRun> {
        for recr in prev_rec
            .runs
            .iter()
            .filter_map(|x| x.as_ref())
            .chain(prev_rec.inc_runs.iter())
        {
            if *ovr == recr.ovr {
                return Some(recr);
            }
        }
        None
    }

    fn set_prot_size_range(
        pjob: &mut ProtectionJob,
        stor_rec: &StorageRecord,
        stor_res: &StorageResult,
    ) {
        // Probe between a bit below the memory share and storage probed size.
        match &mut pjob.scenarios[0] {
            protection::Scenario::MemHogTune(tune) => {
                tune.size_range = (stor_rec.mem.share * 4 / 5, stor_res.mem_size);
            }
            _ => panic!("Unknown protection scenario"),
        }
    }

    fn run_one(
        rctx: &mut RunCtx,
        sjob: &mut StorageJob,
        pjob: &mut ProtectionJob,
        qos_cfg: &IoCostQoSCfg,
        nr_stor_retries: u32,
    ) -> Result<IoCostQoSRecordRun> {
        let started_at = unix_now();

        // Run the storage bench.
        let mut tries = 0;
        let rec_json = loop {
            tries += 1;
            qos_cfg.apply(rctx)?;
            let r = sjob.clone().run(rctx.disable_zswap());
            rctx.stop_agent();
            match r {
                Ok(r) => break r,
                Err(e) => {
                    if prog_exiting() {
                        return Err(e);
                    }
                    if tries > nr_stor_retries {
                        return Err(e.context("Storage benchmark failed too many times"));
                    }
                    warn!(
                        "iocost-qos: Storage benchmark failed ({:#}), retrying...",
                        &e
                    );
                }
            }
        };

        // Acquire storage record and result. We need the result too because
        // it determines how the protection benchmark is run.
        let stor_rec = parse_json_value_or_dump::<StorageRecord>(rec_json.clone())
            .context("Parsing storage record")?;
        let stor_res = parse_json_value_or_dump::<StorageResult>(
            sjob.study(rctx, rec_json)
                .context("Studying storage record")?,
        )
        .context("Parsing storage result")?;

        // Run the protection bench with the hashd params committed by the
        // storage bench.
        qos_cfg.apply(rctx)?;
        Self::set_prot_size_range(pjob, &stor_rec, &stor_res);

        let out = pjob.run(rctx.disable_zswap());
        rctx.stop_agent();

        let prot_rec = match out {
            Ok(r) => parse_json_value_or_dump::<ProtectionRecord>(r)
                .context("Parsing protection record")
                .unwrap(),
            Err(e) => {
                warn!("iocost-qos: Protection benchmark failed ({:#})", &e);
                ProtectionRecord::default()
            }
        };

        let qos = if qos_cfg.ovr.off {
            None
        } else {
            Some(rctx.access_agent_files(|af| af.bench.data.iocost.qos.clone()))
        };

        Ok(IoCostQoSRecordRun {
            period: (started_at, unix_now()),
            ovr: qos_cfg.ovr.clone(),
            qos,
            stor: stor_rec,
            prot: prot_rec,
        })
    }

    fn study_one(
        &self,
        rctx: &mut RunCtx,
        recr: &IoCostQoSRecordRun,
    ) -> Result<IoCostQoSResultRun> {
        let sres: StorageResult = parse_json_value_or_dump(
            self.stor_job
                .study(rctx, serde_json::to_value(&recr.stor).unwrap())
                .context("Studying storage record")?,
        )
        .context("Parsing storage result")?;

        let pres: ProtectionResult = parse_json_value_or_dump(
            self.prot_job
                .study(rctx, serde_json::to_value(&recr.prot).unwrap())
                .context("Studying protection record")?,
        )
        .context("Parsing protection result")?;

        // These are trivial to calculate but cumbersome to access. Let's
        // cache the results.
        let (adjusted_mem_size, adjusted_mem_offload_factor, adjusted_mem_offload_delta) =
            if recr.prot.scenarios.len() > 0 {
                let trec = recr.prot.scenarios[0].as_mem_hog_tune().unwrap();
                match trec.final_size {
                    Some(final_size) => {
                        let amof = final_size as f64 / sres.mem_usage as f64;
                        (
                            Some(final_size),
                            Some(amof),
                            Some(sres.mem_offload_factor - amof),
                        )
                    }
                    None => (None, None, None),
                }
            } else {
                (None, None, None)
            };

        // Study the vrate and IO latency distributions across all the runs.
        let mut study_vrate = StudyMeanPcts::new(|arg| vec![arg.rep.iocost.vrate], None);
        let mut study_read_lat_pcts = StudyIoLatPcts::new("read", None);
        let mut study_write_lat_pcts = StudyIoLatPcts::new("write", None);
        let nr_reports = Studies::new()
            .add(&mut study_vrate)
            .add_multiple(&mut study_read_lat_pcts.studies())
            .add_multiple(&mut study_write_lat_pcts.studies())
            .run(rctx, recr.period)?;

        let vrate = study_vrate.result(Some(&Self::VRATE_PCTS));
        let iolat = [
            study_read_lat_pcts.result(None),
            study_write_lat_pcts.result(None),
        ];

        Ok(IoCostQoSResultRun {
            stor: sres,
            prot: pres,
            adjusted_mem_size,
            adjusted_mem_offload_factor,
            adjusted_mem_offload_delta,
            vrate,
            iolat,
            nr_reports,
        })
    }
}

impl Job for IoCostQoSJob {
    fn sysreqs(&self) -> BTreeSet<SysReq> {
        let mut sysreqs = StorageJob::default().sysreqs();
        sysreqs.append(&mut ProtectionJob::default().sysreqs());
        sysreqs
    }

    fn run(&mut self, rctx: &mut RunCtx) -> Result<serde_json::Value> {
        // We'll be changing bench params mutliples times, revert when done.
        rctx.set_revert_bench();

        // Make sure we have iocost parameters available.
        let mut bench_knobs = rctx.bench_knobs().clone();
        if bench_knobs.iocost_seq == 0 {
            rctx.maybe_run_nested_iocost_params()?;
            bench_knobs = rctx.bench_knobs().clone();
        }

        let (prev_matches, mut prev_rec) = match rctx.prev_job_data() {
            Some(pd) => {
                let prec: IoCostQoSRecord = pd.parse_record()?;
                (
                    self.prev_matches(&prec, rctx.mem_info().profile, &bench_knobs),
                    prec,
                )
            }
            None => (
                true,
                IoCostQoSRecord {
                    base_model: bench_knobs.iocost.model.clone(),
                    base_qos: bench_knobs.iocost.qos.clone(),
                    dither_dist: self.dither_dist,
                    ..Default::default()
                },
            ),
        };

        // Mark the ones with too low a max rate to run.
        if !self.ign_min_perf {
            let abs_min_vrate = iocost_min_vrate(&bench_knobs.iocost.model);
            for ovr in self.runs.iter_mut() {
                ovr.skip_or_adj(abs_min_vrate);
            }
        }

        // Print out what to do beforehand so that the user can spot errors
        // without waiting for the benches to run.
        let mut nr_to_run = 0;
        for (i, ovr) in self.runs.iter().enumerate() {
            let qos_cfg = IoCostQoSCfg::new(&bench_knobs.iocost.qos, ovr);
            let mut skip = false;
            let mut extra_state = " ";
            if ovr.skip {
                skip = true;
                extra_state = "s";
            } else if ovr.min_adj {
                extra_state = "a";
            }

            let new = if !skip && Self::find_matching_rec_run(&ovr, &prev_rec).is_none() {
                nr_to_run += 1;
                true
            } else {
                false
            };

            info!(
                "iocost-qos[{:02}]: {}{} {}",
                i,
                if new { "+" } else { "-" },
                extra_state,
                qos_cfg.format(),
            );
        }

        if nr_to_run > 0 {
            if prev_matches || nr_to_run == self.runs.len() {
                info!(
                    "iocost-qos: {} storage and protection bench sets to run, isol-{} >= {}%",
                    nr_to_run,
                    self.isol_pct,
                    format_pct(self.isol_thr),
                );
            } else {
                bail!(
                    "iocost-qos: {} bench sets to run but existing result doesn't match \
                     the current configuration, consider removing the result file",
                    nr_to_run
                );
            }
        } else {
            info!("iocost-qos: All results are available in the result file, nothing to do");
        }

        let mut runs = vec![];
        for (i, ovr) in self.runs.iter().enumerate() {
            let qos_cfg = IoCostQoSCfg::new(&bench_knobs.iocost.qos, ovr);
            if let Some(recr) = Self::find_matching_rec_run(&ovr, &prev_rec) {
                runs.push(Some(recr.clone()));
                continue;
            } else if ovr.skip {
                runs.push(None);
                continue;
            }

            info!(
                "iocost-qos[{:02}]: Running storage benchmark with QoS parameters:",
                i
            );
            info!("iocost-qos[{:02}]: {}", i, qos_cfg.format());

            loop {
                let mut sjob = self.stor_job.clone();
                sjob.loops = match i {
                    0 => self.stor_base_loops,
                    _ => self.stor_loops,
                };
                let mut pjob = self.prot_job.clone();

                match Self::run_one(rctx, &mut sjob, &mut pjob, &qos_cfg, self.retries) {
                    Ok(recr) => {
                        // Sanity check QoS params.
                        if recr.qos.is_some() {
                            let target_qos = qos_cfg.calc();
                            if recr.qos != target_qos {
                                bail!(
                                    "iocost-qos: result qos ({}) != target qos ({})",
                                    &recr.qos.as_ref().unwrap(),
                                    target_qos.as_ref().unwrap(),
                                );
                            }
                        }
                        prev_rec.inc_runs.push(recr.clone());
                        rctx.update_incremental_record(serde_json::to_value(&prev_rec).unwrap());
                        runs.push(Some(recr));
                        break;
                    }
                    Err(e) => {
                        if !self.allow_fail || prog_exiting() {
                            error!("iocost-qos[{:02}]: Failed ({:#}), giving up...", i, &e);
                            return Err(e);
                        }
                        error!("iocost-qos[{:02}]: Failed ({:#}), skipping...", i, &e);
                        runs.push(None);
                    }
                }
            }
        }

        // We could have broken out early due to allow_fail, pad it to the
        // configured number of runs.
        runs.resize(self.runs.len(), None);

        Ok(serde_json::to_value(&IoCostQoSRecord {
            base_model: bench_knobs.iocost.model,
            base_qos: bench_knobs.iocost.qos,
            mem_profile: rctx.mem_info().profile,
            runs,
            dither_dist: self.dither_dist,
            inc_runs: vec![],
        })
        .unwrap())
    }

    fn study(&self, rctx: &mut RunCtx, rec_json: serde_json::Value) -> Result<serde_json::Value> {
        let rec: IoCostQoSRecord = parse_json_value_or_dump(rec_json)?;

        let mut runs = vec![];
        for recr in rec.runs.iter() {
            match recr {
                Some(recr) => runs.push(Some(self.study_one(rctx, recr)?)),
                None => runs.push(None),
            }
        }

        Ok(serde_json::to_value(&IoCostQoSResult { runs }).unwrap())
    }

    fn format<'a>(
        &self,
        out: &mut Box<dyn Write + 'a>,
        data: &JobData,
        opts: &FormatOpts,
        props: &JobProps,
    ) -> Result<()> {
        let mut sub_full = false;
        for (k, v) in props[0].iter() {
            match k.as_ref() {
                "sub-full" => sub_full = v.len() == 0 || v.parse::<bool>()?,
                k => bail!("unknown format parameter {:?}", k),
            }
        }

        let rec: IoCostQoSRecord = data.parse_record()?;
        let res: IoCostQoSResult = data.parse_result()?;
        assert!(rec.runs.len() == res.runs.len());

        if rec.runs.len() == 0
            || rec.runs[0].is_none()
            || rec.runs[0].as_ref().unwrap().qos.is_some()
        {
            error!("iocost-qos: Failed to format due to missing baseline");
            return Ok(());
        }
        let base_stor_rec = &rec.runs[0].as_ref().unwrap().stor;
        let base_stor_res = &res.runs[0].as_ref().unwrap().stor;

        self.stor_job
            .format_header(out, base_stor_rec, base_stor_res, false);

        if opts.full {
            for (i, (recr, resr)) in rec.runs.iter().zip(res.runs.iter()).enumerate() {
                if recr.is_none() {
                    continue;
                }
                let (recr, resr) = (recr.as_ref().unwrap(), resr.as_ref().unwrap());
                let qos_cfg = IoCostQoSCfg::new(&rec.base_qos, &recr.ovr);

                writeln!(
                    out,
                    "\n\n{}\nQoS: {}\n",
                    &double_underline(&format!("RUN {:02}", i)),
                    qos_cfg.format()
                )
                .unwrap();
                writeln!(out, "{}", underline(&format!("RUN {:02} - Storage", i))).unwrap();

                self.stor_job.format_result(
                    out,
                    &recr.stor,
                    &resr.stor,
                    false,
                    &FormatOpts {
                        full: sub_full,
                        ..*opts
                    },
                );

                let mut pjob = self.prot_job.clone();
                Self::set_prot_size_range(&mut pjob, &recr.stor, &resr.stor);
                pjob.format_result(
                    out,
                    &recr.prot,
                    &resr.prot,
                    &FormatOpts {
                        full: sub_full,
                        ..*opts
                    },
                    &format!("RUN {:02} - Protection ", i),
                );

                writeln!(out, "\n{}", underline(&format!("RUN {:02} - Result", i))).unwrap();

                StudyIoLatPcts::format_rw(out, &resr.iolat, opts, None);

                if recr.qos.is_some() {
                    let mut cnt = 0;
                    write!(out, "\nvrate:").unwrap();
                    for pct in Self::VRATE_PCTS {
                        write!(
                            out,
                            " p{}={:.2}",
                            pct,
                            resr.vrate.get(&pct.to_string()).unwrap()
                        )
                        .unwrap();
                        cnt += 1;
                        if cnt % 7 == 0 {
                            write!(out, "\n      ").unwrap();
                        }
                    }
                    writeln!(out, "\n").unwrap();

                    writeln!(
                        out,
                        "QoS result: MOF={:.3}@{}({:.3}x) vrate={:.2}:{:.2} missing={}%",
                        resr.stor.mem_offload_factor,
                        recr.stor.mem.profile,
                        resr.stor.mem_offload_factor / base_stor_res.mem_offload_factor,
                        resr.vrate["mean"],
                        resr.vrate["stdev"],
                        format_pct(Studies::reports_missing(resr.nr_reports)),
                    )
                    .unwrap();

                    if let Some(amof) = resr.adjusted_mem_offload_factor {
                        let tune_res = match &resr.prot.scenarios[0] {
                            protection::ScenarioResult::MemHogTune(tune_res) => tune_res,
                            _ => bail!("Unknown protection result: {:?}", resr.prot.scenarios[0]),
                        };
                        let hog = tune_res.final_run.as_ref().unwrap();

                        writeln!(
                            out,
                            "            aMOF={:.3}@{}({:.3}x) isol-{}={}% lat_imp={}%:{} work_csv={}%",
                            amof,
                            recr.stor.mem.profile,
                            amof / base_stor_res.mem_offload_factor,
                            &self.isol_pct,
                            format_pct(hog.isol[&self.isol_pct]),
                            format_pct(hog.lat_imp["mean"]),
                            format_pct(hog.lat_imp["stdev"]),
                            format_pct(hog.work_csv),
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            out,
                            "            aMOF=FAIL isol=FAIL lat_imp=FAIL work_csv=FAIL"
                        )
                        .unwrap();
                    }
                }
            }

            writeln!(out, "\n\n{}", double_underline("Summary")).unwrap();
        } else {
            writeln!(out, "").unwrap();
        }

        for (i, ovr) in self.runs.iter().enumerate() {
            let qos_cfg = IoCostQoSCfg::new(&rec.base_qos, ovr);
            write!(out, "[{:02}] QoS: {}", i, qos_cfg.format()).unwrap();
            if ovr.off {
                writeln!(out, " mem_profile={}", rec.mem_profile).unwrap();
            } else {
                writeln!(out, "").unwrap();
            }
        }

        writeln!(out, "").unwrap();
        writeln!(
            out,
            "         MOF     aMOF  isol-{}%       lat-imp%  work-csv%  missing%",
            &self.isol_pct
        )
        .unwrap();

        for (i, resr) in res.runs.iter().enumerate() {
            match resr {
                Some(resr) => {
                    write!(out, "[{:02}] {:>7.3}  ", i, resr.stor.mem_offload_factor).unwrap();
                    if resr.adjusted_mem_offload_factor.is_some() {
                        let hog = resr.prot.scenarios[0]
                            .as_mem_hog_tune()
                            .unwrap()
                            .final_run
                            .as_ref()
                            .unwrap();

                        writeln!(
                            out,
                            "{:>7.3}     {:>5.1}  {:>6.1}:{:>6.1}      {:>5.1}     {:>5.1}",
                            resr.adjusted_mem_offload_factor.unwrap(),
                            hog.isol[&self.isol_pct] * 100.0,
                            hog.lat_imp["mean"] * TO_PCT,
                            hog.lat_imp["stdev"] * TO_PCT,
                            hog.work_csv * TO_PCT,
                            Studies::reports_missing(resr.nr_reports) * TO_PCT,
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            out,
                            "{:>7}     {:>5}  {:>6}:{:>6}      {:>5}     {:>5.1}",
                            "FAIL",
                            "-",
                            "-",
                            "-",
                            "-",
                            Studies::reports_missing(resr.nr_reports) * TO_PCT,
                        )
                        .unwrap()
                    }
                }
                None => writeln!(out, "[{:02}]  SKIP", i).unwrap(),
            }
        }

        let mut format_iolat = |rw, title| {
            writeln!(out, "").unwrap();
            writeln!(
                out,
                "{:17}  p50                p90                p99                max",
                title
            )
            .unwrap();

            for (i, resr) in res.runs.iter().enumerate() {
                match resr {
                    Some(resr) => {
                        let iolat: &BTreeMap<String, BTreeMap<String, f64>> = &resr.iolat[rw];
                        writeln!(
                            out,
                            "[{:02}] {:>5}:{:>5}/{:>5}  {:>5}:{:>5}/{:>5}  \
                              {:>5}:{:>5}/{:>5}  {:>5}:{:>5}/{:>5}",
                            i,
                            format_duration(iolat["50"]["mean"]),
                            format_duration(iolat["50"]["stdev"]),
                            format_duration(iolat["50"]["100"]),
                            format_duration(iolat["90"]["mean"]),
                            format_duration(iolat["90"]["stdev"]),
                            format_duration(iolat["90"]["100"]),
                            format_duration(iolat["99"]["mean"]),
                            format_duration(iolat["99"]["stdev"]),
                            format_duration(iolat["99"]["100"]),
                            format_duration(iolat["100"]["mean"]),
                            format_duration(iolat["100"]["stdev"]),
                            format_duration(iolat["100"]["100"])
                        )
                        .unwrap();
                    }
                    None => writeln!(out, "[{:02}]  Skipped", i).unwrap(),
                }
            }
        };

        format_iolat(READ, "RLAT");
        format_iolat(WRITE, "WLAT");

        Ok(())
    }
}
