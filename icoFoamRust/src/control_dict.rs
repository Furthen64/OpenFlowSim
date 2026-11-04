//! Typed representation of system/controlDict.

use crate::dict::{self, DictError};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ControlDict {
    pub start_time: f64,
    pub end_time: f64,
    pub delta_t: f64,
    pub write_control: WriteControl,
    pub write_interval: usize,
    pub write_format: String,
    pub purge_write: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WriteControl {
    TimeStep,
    Time,
    RunTime,
    Adjustment,
    None,
}

impl ControlDict {
    pub fn read(path: &Path) -> Result<Self, DictError> {
        let d = dict::read_file(path)?;

        let start_time = d.get_f64("startTime").unwrap_or(0.0);
        let end_time = d
            .get_f64("endTime")
            .ok_or_else(|| parse_err(path, "missing endTime"))?;
        let delta_t = d
            .get_f64("deltaT")
            .ok_or_else(|| parse_err(path, "missing deltaT"))?;

        let write_control = match d.get_str("writeControl").unwrap_or("timeStep") {
            "time" => WriteControl::Time,
            "runTime" => WriteControl::RunTime,
            "adjustableRunTime" | "adjustment" => WriteControl::Adjustment,
            "none" => WriteControl::None,
            _ => WriteControl::TimeStep,
        };

        let write_interval = d.get_usize("writeInterval").unwrap_or(1);
        let write_format = d.get_str("writeFormat").unwrap_or("ascii").to_string();
        let purge_write = d.get_usize("purgeWrite").unwrap_or(0);

        Ok(Self {
            start_time,
            end_time,
            delta_t,
            write_control,
            write_interval,
            write_format,
            purge_write,
        })
    }

    pub fn num_steps(&self) -> usize {
        ((self.end_time - self.start_time) / self.delta_t).ceil() as usize
    }

    pub fn should_write(&self, step: usize) -> bool {
        match self.write_control {
            WriteControl::TimeStep => step % self.write_interval == 0,
            _ => step % self.write_interval == 0,
        }
    }
}

fn parse_err(path: &Path, detail: &str) -> DictError {
    DictError::Parse {
        path: path.to_path_buf(),
        pos: 0,
        detail: detail.to_string(),
    }
}
