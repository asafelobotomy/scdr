//! CPU pressure (PSI) with hysteresis. Above `[pressure] high` the
//! background role turns on for apps that aren't focused; below `low` it
//! turns off again. Only sessions with an agent are affected, since without
//! one scdr can't tell which app is focused.

use super::Service;
use super::roles::{app_slice, cgroup_pids};

/// `some avg10` from /proc/pressure/cpu, in percent.
pub fn cpu_some_avg10() -> Option<f64> {
    parse_avg10(&std::fs::read_to_string("/proc/pressure/cpu").ok()?)
}

fn parse_avg10(s: &str) -> Option<f64> {
    let line = s.lines().find(|l| l.starts_with("some "))?;
    line.split_ascii_whitespace()
        .find_map(|f| f.strip_prefix("avg10="))?
        .parse()
        .ok()
}

impl Service {
    pub fn set_cpu_pressure(&mut self, avg10: f64) {
        self.cpu_pressure = Some(avg10);
        let p = self.config.pressure;
        let active = if self.pressure_active { avg10 > p.low } else { avg10 >= p.high };
        if active == self.pressure_active {
            return;
        }
        self.pressure_active = active;
        tracing::info!(
            "CPU pressure {avg10:.1}%: background role {}",
            if active { "on" } else { "off" }
        );
        let uids: Vec<u32> = self.sessions.keys().copied().collect();
        let pids = uids.into_iter().flat_map(|uid| cgroup_pids(app_slice(uid).trim_end_matches('/'))).collect();
        self.reevaluate(pids);
    }

    pub fn cpu_pressure(&self) -> (Option<f64>, bool) {
        (self.cpu_pressure, self.pressure_active)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_psi() {
        let s = "some avg10=12.34 avg60=0.13 avg300=0.38 total=487587535\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        assert_eq!(super::parse_avg10(s), Some(12.34));
        assert_eq!(super::parse_avg10(""), None);
    }
}
