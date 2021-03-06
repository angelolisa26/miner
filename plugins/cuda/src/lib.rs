#[macro_use]
extern crate kaspa_miner;

use clap::{ArgMatches, FromArgMatches};
use cust::prelude::*;
use kaspa_miner::{Plugin, Worker, WorkerSpec};
use log::LevelFilter;
use log::{error, info};
use nvml_wrapper::Device as NvmlDevice;
use nvml_wrapper::Nvml;
use std::error::Error as StdError;

pub type Error = Box<dyn StdError + Send + Sync + 'static>;

mod cli;
mod worker;

use crate::cli::CudaOpt;
use crate::worker::CudaGPUWorker;

const DEFAULT_WORKLOAD_SCALE: f32 = 256.;

pub struct CudaPlugin {
    specs: Vec<CudaWorkerSpec>,
    nvml_instance: Nvml,
    _enabled: bool,
}

impl CudaPlugin {
    fn new() -> Result<Self, Error> {
        cust::init(CudaFlags::empty())?;
        env_logger::builder().filter_level(LevelFilter::Info).parse_default_env().init();
        let nvml_instance = Nvml::init()?;
        Ok(Self { specs: Vec::new(), _enabled: false, nvml_instance })
    }
}

impl Plugin for CudaPlugin {
    fn name(&self) -> &'static str {
        "CUDA Worker"
    }

    fn enabled(&self) -> bool {
        self._enabled
    }

    fn get_worker_specs(&self) -> Vec<Box<dyn WorkerSpec>> {
        self.specs.iter().map(|spec| Box::new(*spec) as Box<dyn WorkerSpec>).collect::<Vec<Box<dyn WorkerSpec>>>()
    }

    //noinspection RsTypeCheck
    fn process_option(&mut self, matches: &ArgMatches) -> Result<(), kaspa_miner::Error> {
        let opts: CudaOpt = CudaOpt::from_arg_matches(matches)?;

        self._enabled = !opts.cuda_disable;

        let gpus: Vec<u16> = match &opts.cuda_device {
            Some(devices) => devices.clone(),
            None => {
                let gpu_count = Device::num_devices().unwrap() as u16;
                (0..gpu_count).collect()
            }
        };

        // if any of cuda_lock_core_clocks / cuda_lock_mem_clocks / cuda_power_limit is valid, init nvml and try to apply
        if opts.cuda_lock_core_clocks.is_some()
            || opts.cuda_lock_mem_clocks.is_some()
            || opts.cuda_power_limits.is_some()
        {
            for i in 0..gpus.len() {
                let lock_mem_clock: Option<u32> = match &opts.cuda_lock_mem_clocks {
                    Some(mem_clocks) if i < mem_clocks.len() => Some(mem_clocks[i]),
                    Some(mem_clocks) if !mem_clocks.is_empty() => Some(*mem_clocks.last().unwrap()),
                    _ => None,
                };

                let lock_core_clock: Option<u32> = match &opts.cuda_lock_core_clocks {
                    Some(core_clocks) if i < core_clocks.len() => Some(core_clocks[i]),
                    Some(core_clocks) if !core_clocks.is_empty() => Some(*core_clocks.last().unwrap()),
                    _ => None,
                };

                let power_limit: Option<u32> = match &opts.cuda_power_limits {
                    Some(power_limits) if i < power_limits.len() => Some(power_limits[i]),
                    Some(power_limits) if !power_limits.is_empty() => Some(*power_limits.last().unwrap()),
                    _ => None,
                };

                let mut nvml_device: NvmlDevice = self.nvml_instance.device_by_index(gpus[i] as u32)?;

                if let Some(lmc) = lock_mem_clock {
                    match nvml_device.set_mem_locked_clocks(lmc, lmc) {
                        Err(e) => error!("set mem locked clocks {:?}", e),
                        _ => info!("GPU #{} #{} lock mem clock at {} Mhz", i, &nvml_device.name()?, &lmc),
                    };
                }

                if let Some(lcc) = lock_core_clock {
                    match nvml_device.set_gpu_locked_clocks(lcc, lcc) {
                        Err(e) => error!("set gpu locked clocks {:?}", e),
                        _ => info!("GPU #{} #{} lock core clock at {} Mhz", i, &nvml_device.name()?, &lcc),
                    };
                };

                if let Some(pl) = power_limit {
                    match nvml_device.set_power_management_limit(pl * 1000) {
                        Err(e) => error!("set power limit {:?}", e),
                        _ => info!("GPU #{} #{} power limit at {} W", i, &nvml_device.name()?, &pl),
                    };
                };
            }
        }

        self.specs = (0..gpus.len())
            .map(|i| CudaWorkerSpec {
                device_id: gpus[i] as u32,
                workload: match &opts.cuda_workload {
                    Some(workload) if i < workload.len() => workload[i],
                    Some(workload) if !workload.is_empty() => *workload.last().unwrap(),
                    _ => DEFAULT_WORKLOAD_SCALE,
                },
                is_absolute: opts.cuda_workload_absolute,
                blocking_sync: !opts.cuda_no_blocking_sync,
            })
            .collect();
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct CudaWorkerSpec {
    device_id: u32,
    workload: f32,
    is_absolute: bool,
    blocking_sync: bool,
}

impl WorkerSpec for CudaWorkerSpec {
    fn build(&self) -> Box<dyn Worker> {
        Box::new(CudaGPUWorker::new(self.device_id, self.workload, self.is_absolute, self.blocking_sync).unwrap())
    }
}

declare_plugin!(CudaPlugin, CudaPlugin::new, CudaOpt);
