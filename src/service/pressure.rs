//! CPU pressure (PSI) with hysteresis. Above `[pressure] high` the
//! background role turns on for apps that aren't focused; below `low` it
//! turns off again, but never sooner than `hold_secs` after the last
//! switch. Only sessions with an agent are affected, since without
//! one scdr can't tell which app is focused.

use super::Service;
use super::roles::{app_slice, cgroup_pids};

/// `some avg10` from /proc/pressure/cpu, in percent.
pub fn cpu_some_avg10() -> Option<f64> {
    crate::system::psi_some_avg10("cpu")
}

impl Service {
    pub fn set_cpu_pressure(&mut self, avg10: f64) {
        self.cpu_pressure = Some(avg10);
        let p = self.config.pressure;
        let active = if self.pressure_active { avg10 > p.low } else { avg10 >= p.high };
        if active == self.pressure_active {
            return;
        }
        // Short spikes would otherwise flip every app's unit weights back and forth.
        let hold = std::time::Duration::from_secs(p.hold_secs);
        if self.pressure_switched.is_some_and(|t| t.elapsed() < hold) {
            return;
        }
        self.pressure_active = active;
        self.pressure_switched = Some(std::time::Instant::now());
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
    fn service(hold_secs: u64) -> super::Service {
        let toml = format!("[pressure]\nhold_secs = {hold_secs}\n");
        let config = crate::config::Config::from_sources(vec![("p.toml".into(), toml)]).unwrap();
        super::Service::new(config, true)
    }

    #[test]
    fn hysteresis_and_hold() {
        let mut s = service(0);
        s.set_cpu_pressure(25.0);
        assert!(s.cpu_pressure().1, "above high");
        s.set_cpu_pressure(10.0);
        assert!(s.cpu_pressure().1, "between low and high keeps it on");
        s.set_cpu_pressure(2.0);
        assert!(!s.cpu_pressure().1, "below low");

        let mut s = service(3600);
        s.set_cpu_pressure(25.0);
        assert!(s.cpu_pressure().1, "the first switch isn't held");
        s.set_cpu_pressure(2.0);
        assert!(s.cpu_pressure().1, "held on until hold_secs pass");
    }

    #[test]
    fn parses_psi() {
        let s = "some avg10=12.34 avg60=0.13 avg300=0.38 total=487587535\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        assert_eq!(crate::system::parse_psi_some_avg10(s), Some(12.34));
        assert_eq!(crate::system::parse_psi_some_avg10(""), None);
    }
}
