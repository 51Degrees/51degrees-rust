/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! Parallel groups under load, against the same elements added one at a
//! time.
//!
//! ```text
//! cargo bench -p fiftyone-pipeline-core --bench group_load -- [options]
//! ```
//!
//! Each client thread processes one flow data after another, so the load
//! rises with the number of clients. Each line of output is one run, as
//! CSV, giving flow data a second, the 50th, 90th and 99th percentile times,
//! and the process's CPU as a share of every logical processor (empty where
//! the platform gives none). The last lines give the median of the rounds
//! for each case.
//!
//! Options, each followed by its value:
//!
//! - `--groups`: any of `production`, `nothing-2`, `nothing-4`, `nothing-7`,
//!   `waiting-2` and `busy-2`, separated by commas. The default is
//!   `production,nothing-7,waiting-2,busy-2`.
//! - `--patterns`: any of `sequence`, `group` and `threads`. The default is
//!   `sequence,group`.
//! - `--clients`: the numbers of client threads. The default is
//!   `1,12,24,48`.
//! - `--seconds`: the time measured in each run. The default is 1.5.
//! - `--warm-seconds`: the time each run processes before it measures. The
//!   default is 0.3.
//! - `--rounds`: how many times every case runs. Each round runs every case
//!   once, so drift falls on every case alike. The default is 3.
//! - `--wait-microseconds`: how long each `waiting-2` element blocks its
//!   thread. The default is 1000.
//!
//! The cost of a group on an idle machine, with and without the shared
//! threads:
//!
//! ```text
//! cargo bench -p fiftyone-pipeline-core --bench group_load -- \
//!   --groups nothing-2,nothing-4,nothing-7 \
//!   --patterns sequence,group,threads --clients 1
//! ```

use std::any::Any;
use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PipelineBuilder, PropertyMetaData, PropertyValue,
    Result, TypedKey,
};

/// The data keys of the elements, one for each element of the largest group.
const KEYS: [&str; 7] = ["e0", "e1", "e2", "e3", "e4", "e5", "e6"];

/// How long each element of `production` keeps the processor busy. These are
/// the median times of the seven elements of a group in a production .NET
/// pipeline, the longest two being Device Detection and IP Intelligence.
const PRODUCTION_MICROSECONDS: [f64; 7] = [41.6, 133.8, 1.2, 1.2, 4.6, 31.4, 49.9];

/// How long each element of `busy-2` keeps the processor busy.
const BUSY_MICROSECONDS: f64 = 500.0;

const DEFAULT_GROUPS: &str = "production,nothing-7,waiting-2,busy-2";
const DEFAULT_PATTERNS: &str = "sequence,group";
const DEFAULT_CLIENTS: &str = "1,12,24,48";
const DEFAULT_SECONDS: f64 = 1.5;
const DEFAULT_WARM_SECONDS: f64 = 0.3;
const DEFAULT_ROUNDS: usize = 3;
const DEFAULT_WAIT_MICROSECONDS: f64 = 1000.0;

/// The room each client reserves for its times before it starts.
const TIMES_RESERVED: usize = 1 << 16;

/// The percentiles each run reports.
const PERCENTILES: [f64; 3] = [0.50, 0.90, 0.99];

/// What an element does before it adds its one value.
#[derive(Clone, Copy)]
enum Work {
    Nothing,
    /// Keep the processor busy for this long.
    Busy(Duration),
    /// Block the thread for this long, as a call to another service does.
    Wait(Duration),
}

struct Data(MapElementData);

impl ElementData for Data {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct Element {
    key: &'static str,
    work: Work,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Element {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        match self.work {
            Work::Nothing => {}
            Work::Busy(time) => {
                let started = Instant::now();
                while started.elapsed() < time {
                    std::hint::spin_loop();
                }
            }
            Work::Wait(time) => thread::sleep(time),
        }
        data.get_or_add(TypedKey::<Data>::new(self.key), || {
            Data(MapElementData::new().set("value", "x"))
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// The elements of the group named `name`.
fn group(name: &str, wait: Duration) -> std::result::Result<Vec<Arc<dyn FlowElement>>, String> {
    let busy = |microseconds: f64| Work::Busy(Duration::from_secs_f64(microseconds / 1e6));
    let works: Vec<Work> = match name {
        "production" => PRODUCTION_MICROSECONDS.iter().map(|&m| busy(m)).collect(),
        "nothing-2" => vec![Work::Nothing; 2],
        "nothing-4" => vec![Work::Nothing; 4],
        "nothing-7" => vec![Work::Nothing; 7],
        "waiting-2" => vec![Work::Wait(wait); 2],
        "busy-2" => vec![busy(BUSY_MICROSECONDS); 2],
        _ => return Err(format!("unknown group '{name}'")),
    };
    Ok(works
        .into_iter()
        .zip(KEYS)
        .map(|(work, key)| {
            Arc::new(Element {
                key,
                work,
                filter: EvidenceKeyFilterWhitelist::default(),
            }) as Arc<dyn FlowElement>
        })
        .collect())
}

/// How the elements of a group are run for each flow data.
enum Runner {
    /// One pipeline, its elements added one at a time (`sequence`) or as a
    /// parallel group (`group`).
    Pipeline(Arc<Pipeline>),
    /// Each element in a pipeline of its own, every one but the first on a
    /// new thread (`threads`). This is what the shared group threads avoid.
    Threads(Vec<Arc<Pipeline>>),
}

impl Runner {
    fn new(pattern: &str, elements: &[Arc<dyn FlowElement>]) -> std::result::Result<Self, String> {
        let built = |builder: PipelineBuilder| builder.build().map_err(|e| e.to_string());
        match pattern {
            "sequence" => {
                let builder = elements
                    .iter()
                    .fold(Pipeline::builder(), |b, e| b.add_element(Arc::clone(e)));
                Ok(Runner::Pipeline(built(builder)?))
            }
            "group" => Ok(Runner::Pipeline(built(
                Pipeline::builder().add_parallel_elements(elements.iter().cloned()),
            )?)),
            "threads" => Ok(Runner::Threads(
                elements
                    .iter()
                    .map(|e| built(Pipeline::builder().add_element(Arc::clone(e))))
                    .collect::<std::result::Result<_, _>>()?,
            )),
            _ => Err(format!("unknown pattern '{pattern}'")),
        }
    }

    fn process(&self) {
        match self {
            Runner::Pipeline(pipeline) => {
                pipeline
                    .create_flow_data()
                    .process()
                    .expect("processing failed");
            }
            Runner::Threads(pipelines) => thread::scope(|scope| {
                let (first, others) = pipelines.split_first().expect("a group is never empty");
                let started: Vec<_> = others
                    .iter()
                    .map(|p| scope.spawn(move || p.create_flow_data().process()))
                    .collect();
                first
                    .create_flow_data()
                    .process()
                    .expect("processing failed");
                for handle in started {
                    handle
                        .join()
                        .expect("an element panicked")
                        .expect("processing failed");
                }
            }),
        }
    }
}

/// What one run measured.
struct Measured {
    flow_data: f64,
    per_second: f64,
    /// Microseconds at each of [`PERCENTILES`].
    percentiles: [f64; 3],
    cpu_percent: Option<f64>,
}

/// Process flow data from `clients` threads, warm for `warm`, then measure
/// for `measured`.
fn run(runner: &Runner, clients: usize, warm: Duration, measured: Duration) -> Measured {
    let stop = AtomicBool::new(false);
    let measuring = AtomicBool::new(false);
    let start = Barrier::new(clients + 1);
    let mut nanoseconds: Vec<u64> = Vec::new();
    let mut wall = Duration::ZERO;
    let mut cpu = None;
    thread::scope(|scope| {
        let handles: Vec<_> = (0..clients)
            .map(|_| {
                scope.spawn(|| {
                    let mut times = Vec::with_capacity(TIMES_RESERVED);
                    start.wait();
                    while !stop.load(Ordering::Relaxed) {
                        let record = measuring.load(Ordering::Relaxed);
                        let started = Instant::now();
                        runner.process();
                        if record {
                            let took = started.elapsed().as_nanos();
                            times.push(u64::try_from(took).unwrap_or(u64::MAX));
                        }
                    }
                    times
                })
            })
            .collect();
        start.wait();
        thread::sleep(warm);
        let cpu_before = process_cpu();
        let began = Instant::now();
        measuring.store(true, Ordering::Relaxed);
        thread::sleep(measured);
        measuring.store(false, Ordering::Relaxed);
        wall = began.elapsed();
        cpu = process_cpu()
            .zip(cpu_before)
            .map(|(after, before)| after - before);
        stop.store(true, Ordering::Relaxed);
        for handle in handles {
            nanoseconds.extend(handle.join().expect("a client panicked"));
        }
    });
    nanoseconds.sort_unstable();
    let processors = thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    Measured {
        flow_data: nanoseconds.len() as f64,
        per_second: nanoseconds.len() as f64 / wall.as_secs_f64(),
        percentiles: PERCENTILES.map(|q| percentile(&nanoseconds, q) / 1e3),
        cpu_percent: cpu.map(|c| c.as_secs_f64() / wall.as_secs_f64() / processors * 100.0),
    }
}

/// The nearest-rank percentile `q` of the sorted `values`, or 0 if there are
/// none.
fn percentile(values: &[u64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let rank = ((q * values.len() as f64).ceil() as usize).clamp(1, values.len());
    values[rank - 1] as f64
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    }
}

/// The CPU time this process has used, user and kernel together.
#[cfg(windows)]
fn process_cpu() -> Option<Duration> {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    let (mut creation, mut exit, mut kernel, mut user) = Default::default();
    // SAFETY: the pseudo handle needs no closing and every pointer is to a
    // local that lives for the call.
    let done = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    // FILETIME counts 100 nanosecond intervals.
    let ticks = |t: &FileTime| (u64::from(t.high) << 32) | u64::from(t.low);
    (done != 0).then(|| Duration::from_nanos((ticks(&kernel) + ticks(&user)) * 100))
}

/// The CPU time this process has used, user and system together.
#[cfg(unix)]
fn process_cpu() -> Option<Duration> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: the pointer is to a local that lives for the call, and
    // getrusage fills it when it returns 0.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: getrusage returned 0, so it filled the value.
    let usage = unsafe { usage.assume_init() };
    let time = |t: libc::timeval| {
        Duration::from_secs(t.tv_sec as u64) + Duration::from_micros(t.tv_usec as u64)
    };
    Some(time(usage.ru_utime) + time(usage.ru_stime))
}

#[cfg(not(any(windows, unix)))]
fn process_cpu() -> Option<Duration> {
    None
}

struct Options {
    groups: Vec<String>,
    patterns: Vec<String>,
    clients: Vec<usize>,
    measured: Duration,
    warm: Duration,
    rounds: usize,
    wait: Duration,
}

/// The time `value` gives, in units of which `per_second` make a second.
fn time(value: &str, name: &str, per_second: f64) -> std::result::Result<Duration, String> {
    value
        .parse::<f64>()
        .ok()
        .and_then(|t| Duration::try_from_secs_f64(t / per_second).ok())
        .ok_or_else(|| format!("{name} needs a time of 0 or more, not '{value}'"))
}

fn count(value: &str, name: &str) -> std::result::Result<usize, String> {
    match value.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{name} needs whole numbers above 0, not '{value}'")),
    }
}

fn list(value: &str) -> Vec<String> {
    value.split(',').map(|v| v.trim().to_owned()).collect()
}

fn options(mut args: impl Iterator<Item = String>) -> std::result::Result<Options, String> {
    let mut values = BTreeMap::new();
    while let Some(name) = args.next() {
        // cargo bench adds --bench to the arguments it passes.
        if name == "--bench" {
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{name} needs a value"))?;
        values.insert(name, value);
    }
    let mut take = |name: &str, default: String| values.remove(name).unwrap_or(default);
    let groups = list(&take("--groups", DEFAULT_GROUPS.to_owned()));
    let patterns = list(&take("--patterns", DEFAULT_PATTERNS.to_owned()));
    let clients = list(&take("--clients", DEFAULT_CLIENTS.to_owned()))
        .iter()
        .map(|c| count(c, "--clients"))
        .collect::<std::result::Result<_, _>>()?;
    let measured = time(
        &take("--seconds", DEFAULT_SECONDS.to_string()),
        "--seconds",
        1.0,
    )?;
    let warm = time(
        &take("--warm-seconds", DEFAULT_WARM_SECONDS.to_string()),
        "--warm-seconds",
        1.0,
    )?;
    let rounds = count(&take("--rounds", DEFAULT_ROUNDS.to_string()), "--rounds")?;
    let wait = time(
        &take("--wait-microseconds", DEFAULT_WAIT_MICROSECONDS.to_string()),
        "--wait-microseconds",
        1e6,
    )?;
    if let Some(unknown) = values.keys().next() {
        return Err(format!("unknown option {unknown}"));
    }
    if measured.is_zero() {
        return Err("--seconds needs a time above 0".to_owned());
    }
    Ok(Options {
        groups,
        patterns,
        clients,
        measured,
        warm,
        rounds,
        wait,
    })
}

fn main() -> ExitCode {
    match measure() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}. The options are described at the top of benches/group_load.rs.");
            ExitCode::from(2)
        }
    }
}

fn measure() -> std::result::Result<(), String> {
    let options = options(std::env::args().skip(1))?;
    let mut groups = Vec::new();
    for name in &options.groups {
        let elements = group(name, options.wait)?;
        let runners = options
            .patterns
            .iter()
            .map(|pattern| Ok((pattern.as_str(), Runner::new(pattern, &elements)?)))
            .collect::<std::result::Result<Vec<_>, String>>()?;
        groups.push((name.as_str(), runners));
    }

    println!("round,group,clients,pattern,flow_data,per_second,p50_us,p90_us,p99_us,cpu_percent");
    // Every pattern runs at each load before the load changes, so drift falls
    // on the patterns alike.
    let mut runs: BTreeMap<(usize, usize, usize), Vec<Measured>> = BTreeMap::new();
    for round in 1..=options.rounds {
        for (g, (name, runners)) in groups.iter().enumerate() {
            for (c, &clients) in options.clients.iter().enumerate() {
                for (p, (pattern, runner)) in runners.iter().enumerate() {
                    let m = run(runner, clients, options.warm, options.measured);
                    print_row(&round.to_string(), name, clients, pattern, &m);
                    runs.entry((g, c, p)).or_default().push(m);
                }
            }
        }
    }
    for ((g, c, p), measured) in runs {
        let (name, runners) = &groups[g];
        let middle = |f: fn(&Measured) -> f64| median(measured.iter().map(f).collect());
        let cpu: Option<Vec<f64>> = measured.iter().map(|m| m.cpu_percent).collect();
        let m = Measured {
            flow_data: middle(|m| m.flow_data),
            per_second: middle(|m| m.per_second),
            percentiles: [0, 1, 2]
                .map(|i| median(measured.iter().map(|m| m.percentiles[i]).collect())),
            cpu_percent: cpu.map(median),
        };
        print_row("median", name, options.clients[c], runners[p].0, &m);
    }
    Ok(())
}

fn print_row(round: &str, group: &str, clients: usize, pattern: &str, m: &Measured) {
    let [p50, p90, p99] = m.percentiles;
    let cpu = m.cpu_percent.map_or(String::new(), |c| format!("{c:.1}"));
    println!(
        "{round},{group},{clients},{pattern},{:.0},{:.0},{p50:.2},{p90:.2},{p99:.2},{cpu}",
        m.flow_data, m.per_second
    );
}
