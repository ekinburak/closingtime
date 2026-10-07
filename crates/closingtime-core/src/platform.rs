use crate::{Result, model::*};
#[cfg(target_os = "linux")]
use std::fs;
use std::{collections::BTreeMap, io};
#[cfg(target_os = "macos")]
use std::{collections::BTreeSet, ffi::CStr, process::Command};

/// Backends must distinguish an absent process from an unreadable one.
pub trait Backend {
    fn host(&self) -> &str;
    fn boot(&self) -> &str;
    fn uid(&self) -> u32;
    fn inspect(&self, pid: u32) -> Inspection;
    fn snapshot(&self) -> Result<Snapshot>;
    /// Must bind/check identity before signaling, never fall back to pattern/group kills.
    fn signal(&self, identity: &ProcessIdentity, signal: i32) -> Result<()>;
    fn pidfd_available(&self) -> bool {
        false
    }
}

pub struct NativeBackend {
    host: String,
    boot: String,
    uid: u32,
    /// The supervisor's own systemd cgroup. Descendants that stay inside it were
    /// not placed there by a service manager, even when it is a `.service` unit.
    #[cfg(target_os = "linux")]
    cgroup: Option<String>,
}
impl NativeBackend {
    pub fn new() -> Result<Self> {
        let (host, boot) = platform_identity()?;
        Ok(Self {
            host,
            boot,
            uid: unsafe { libc::geteuid() },
            #[cfg(target_os = "linux")]
            cgroup: fs::read_to_string("/proc/self/cgroup")
                .ok()
                .and_then(|text| systemd_cgroup(&text).map(str::to_owned)),
        })
    }
    pub fn doctor(&self) -> Result<Doctor> {
        let snapshot = self.snapshot()?;
        let current = snapshot.processes.get(&std::process::id());
        let mut warnings = snapshot.warnings;
        if cfg!(target_os = "macos") {
            warnings.push(
                "macOS identity checks and kill are not atomic; cleanup retains a PID reuse race"
                    .into(),
            );
        }
        Ok(Doctor {
            platform: std::env::consts::OS.into(),
            host: self.host.clone(),
            boot: self.boot.clone(),
            uid: self.uid,
            identity_available: current.is_some(),
            tags_readable: current.is_some_and(|p| p.tag_readable),
            ports_available: !warnings
                .iter()
                .any(|w| w.starts_with("TCP inventory unavailable")),
            pidfd_available: self.pidfd_available(),
            warnings,
        })
    }
}
impl Backend for NativeBackend {
    fn host(&self) -> &str {
        &self.host
    }
    fn boot(&self) -> &str {
        &self.boot
    }
    fn uid(&self) -> u32 {
        self.uid
    }
    fn inspect(&self, pid: u32) -> Inspection {
        inspect_native(self, pid)
    }
    fn snapshot(&self) -> Result<Snapshot> {
        let mut snapshot = Snapshot::default();
        for pid in list_pids()? {
            match self.inspect(pid) {
                Inspection::Present(p) => {
                    snapshot.processes.insert(pid, p);
                }
                Inspection::Unavailable(reason) => {
                    snapshot.unavailable.insert(pid, reason);
                }
                Inspection::Gone => {}
            }
        }
        #[cfg(target_os = "macos")]
        {
            let managed = launchd_pids();
            for p in snapshot.processes.values_mut() {
                p.manager_owned = managed.as_ref().map(|pids| pids.contains(&p.identity.pid));
            }
            if managed.is_none() {
                snapshot
                    .warnings
                    .push("launchd inventory unavailable; manager ownership is unknown".into());
            }
        }
        match listening_ports(&snapshot.processes) {
            Ok(ports) => {
                // Port enumeration is a separate OS read. Never attach a recycled
                // PID's socket to the earlier process identity.
                let verified: BTreeMap<_, _> = ports
                    .iter()
                    .map(|port| port.pid)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .map(|pid| {
                        let matches = matches!(
                            self.inspect(pid),
                            Inspection::Present(ref current)
                                if snapshot.processes.get(&pid).is_some_and(|old| old.identity == current.identity)
                        );
                        (pid, matches)
                    })
                    .collect();
                snapshot.ports = ports
                    .into_iter()
                    .filter(|port| verified.get(&port.pid) == Some(&true))
                    .collect();
            }
            Err(e) => snapshot
                .warnings
                .push(format!("TCP inventory unavailable: {e}")),
        }
        Ok(snapshot)
    }
    fn signal(&self, identity: &ProcessIdentity, signal: i32) -> Result<()> {
        if identity.uid != self.uid
            || identity.host != self.host
            || identity.boot != self.boot
            || identity.pid <= 1
        {
            return Err("identity is not a current-user process on this boot".into());
        }
        #[cfg(target_os = "linux")]
        {
            // Open first, inspect second: PID reuse between inspect and open cannot target a new process.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, identity.pid, 0) } as i32;
            if fd < 0 {
                return Err(io::Error::last_os_error().into());
            }
            use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
            let handle = unsafe { OwnedFd::from_raw_fd(fd) };
            ensure_identity(self.inspect(identity.pid), identity)?;
            let result = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    handle.as_raw_fd(),
                    signal,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            if result < 0 {
                return Err(io::Error::last_os_error().into());
            }
        }
        #[cfg(target_os = "macos")]
        {
            ensure_identity(self.inspect(identity.pid), identity)?;
            // Darwin has no pidfd equivalent. This check reduces, but cannot eliminate, the race.
            if unsafe { libc::kill(identity.pid as i32, signal) } < 0 {
                return Err(io::Error::last_os_error().into());
            }
        }
        Ok(())
    }
    fn pidfd_available(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, std::process::id(), 0) } as i32;
            if fd >= 0 {
                unsafe {
                    libc::close(fd);
                }
                return true;
            }
        }
        false
    }
}

fn ensure_identity(inspection: Inspection, expected: &ProcessIdentity) -> Result<()> {
    match inspection {
        Inspection::Present(p) if &p.identity == expected && !p.zombie => Ok(()),
        _ => Err("process is gone, unreadable, reused, or awaiting reaping".into()),
    }
}

fn session_tag(bytes: &[u8]) -> Option<String> {
    let prefix = format!("{SESSION_ENV}=");
    bytes.split(|&b| b == 0).find_map(|item| {
        std::str::from_utf8(item)
            .ok()?
            .strip_prefix(&prefix)
            .map(str::to_owned)
    })
}

#[cfg(target_os = "linux")]
fn platform_identity() -> Result<(String, String)> {
    let machine = fs::read_to_string("/etc/machine-id")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map_or_else(|| fs::read_to_string("/proc/sys/kernel/hostname"), Ok)?;
    // Containers share a kernel boot ID. Never reuse identities across PID namespaces.
    let namespace = fs::read_link("/proc/self/ns/pid")?;
    let host = format!("{}:{}", machine.trim(), namespace.to_string_lossy());
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned();
    if host.is_empty() || boot.is_empty() {
        return Err("host/boot identity unavailable".into());
    }
    Ok((host, boot))
}

#[cfg(target_os = "linux")]
fn list_pids() -> Result<Vec<u32>> {
    Ok(fs::read_dir("/proc")?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect())
}

#[cfg(target_os = "linux")]
fn read_linux_stat(pid: u32) -> io::Result<(String, u32, String, bool)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close = stat
        .rfind(')')
        .ok_or_else(|| io::Error::other("invalid process stat"))?;
    let open = stat
        .find('(')
        .ok_or_else(|| io::Error::other("invalid process stat"))?;
    let fields: Vec<_> = stat[close + 1..].split_whitespace().collect();
    if fields.len() < 20 {
        return Err(io::Error::other("incomplete process stat"));
    }
    Ok((
        stat[open + 1..close].into(),
        fields[1].parse().map_err(io::Error::other)?,
        fields[19].into(),
        matches!(fields[0], "Z" | "X"),
    ))
}

#[cfg(target_os = "linux")]
fn inspect_native(backend: &NativeBackend, pid: u32) -> Inspection {
    let result = (|| -> Result<Process> {
        let (name, parent_pid, start, zombie) = read_linux_stat(pid)?;
        let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
        let uid = status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .ok_or("missing uid")?
            .parse()?;
        let executable = fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        let environment = if uid == backend.uid {
            fs::read(format!("/proc/{pid}/environ")).ok()
        } else {
            None
        };
        let manager_owned = fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .ok()
            .and_then(|text| {
                systemd_cgroup(&text).map(|path| service_managed(path, backend.cgroup.as_deref()))
            });
        if read_linux_stat(pid)?.2 != start {
            return Err("process identity changed during inspection".into());
        }
        Ok(Process {
            identity: ProcessIdentity {
                host: backend.host.clone(),
                boot: backend.boot.clone(),
                uid,
                pid,
                start,
            },
            parent_pid,
            executable,
            name,
            zombie,
            session_tag: environment.as_deref().and_then(session_tag),
            tag_readable: environment.is_some(),
            manager_owned,
        })
    })();
    match result {
        Ok(p) => Inspection::Present(p),
        Err(e)
            if e.downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
        {
            Inspection::Gone
        }
        Err(e) => Inspection::Unavailable(e.to_string()),
    }
}

/// The v1 `name=systemd` hierarchy on legacy/hybrid hosts, else the unified (v2) path.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn systemd_cgroup(text: &str) -> Option<&str> {
    let hierarchy = |name: &str| {
        text.lines().find_map(|line| {
            let mut fields = line.splitn(3, ':');
            fields.next()?;
            (fields.next()? == name).then(|| fields.next()).flatten()
        })
    };
    hierarchy("name=systemd").or_else(|| hierarchy(""))
}

/// A `.service` unit (other than the per-user manager) owns the process, unless it is
/// the supervisor's own unit: a CI runner or SSH service is not a separate manager.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn service_managed(path: &str, own: Option<&str>) -> bool {
    let relative = own
        .filter(|own| *own != "/")
        .and_then(|own| path.strip_prefix(own))
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        .unwrap_or(path);
    relative
        .split('/')
        .any(|part| part.ends_with(".service") && !part.starts_with("user@"))
}

#[cfg(target_os = "linux")]
fn listening_ports(processes: &BTreeMap<u32, Process>) -> Result<Vec<Port>> {
    let mut sockets: BTreeMap<String, (String, u16)> = BTreeMap::new();
    for (path, ipv6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        let text = fs::read_to_string(path)?;
        for line in text.lines().skip(1) {
            let f: Vec<_> = line.split_whitespace().collect();
            if f.len() < 10 || f[3] != "0A" {
                continue;
            }
            let Some((address, port)) = f[1].split_once(':') else {
                continue;
            };
            let port = u16::from_str_radix(port, 16)?;
            let address = if ipv6 {
                let mut bytes = [0u8; 16];
                if address.len() != 32 {
                    continue;
                }
                for i in 0..4 {
                    bytes[i * 4..i * 4 + 4].copy_from_slice(
                        &u32::from_str_radix(&address[i * 8..i * 8 + 8], 16)?.to_ne_bytes(),
                    );
                }
                std::net::Ipv6Addr::from(bytes).to_string()
            } else {
                std::net::Ipv4Addr::from(u32::from_str_radix(address, 16)?.to_ne_bytes())
                    .to_string()
            };
            sockets.insert(f[9].into(), (address, port));
        }
    }
    let uid = unsafe { libc::geteuid() };
    let mut ports = Vec::new();
    for p in processes.values().filter(|p| p.identity.uid == uid) {
        let Ok(fds) = fs::read_dir(format!("/proc/{}/fd", p.identity.pid)) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(link) = fs::read_link(fd.path()) {
                let text = link.to_string_lossy();
                if let Some(inode) = text
                    .strip_prefix("socket:[")
                    .and_then(|s| s.strip_suffix(']'))
                {
                    if let Some((address, port)) = sockets.get(inode) {
                        ports.push(Port {
                            pid: p.identity.pid,
                            address: address.clone(),
                            port: *port,
                        });
                    }
                }
            }
        }
    }
    ports.sort_by_key(|p| (p.pid, p.port, p.address.clone()));
    ports.dedup();
    Ok(ports)
}

#[cfg(target_os = "macos")]
fn sysctl_string(name: &CStr) -> Result<String> {
    let mut buffer = vec![0u8; 256];
    let mut len = buffer.len();
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    buffer.truncate(len);
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    let result = String::from_utf8(buffer)?;
    if result.is_empty() {
        return Err("empty kernel identity".into());
    }
    Ok(result)
}

#[cfg(target_os = "macos")]
fn platform_identity() -> Result<(String, String)> {
    let mut uuid = [0u8; 16];
    let timeout = libc::timespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    if unsafe { libc::gethostuuid(uuid.as_mut_ptr(), &timeout) } < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let host = uuid.iter().map(|b| format!("{b:02x}")).collect();
    Ok((host, sysctl_string(c"kern.bootsessionuuid")?))
}

#[cfg(target_os = "macos")]
fn list_pids() -> Result<Vec<u32>> {
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut pids = vec![0i32; count as usize + 1024];
    let count =
        unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
    if count < 0 {
        return Err(io::Error::last_os_error().into());
    }
    pids.truncate(count as usize);
    Ok(pids
        .into_iter()
        .filter(|p| *p > 0)
        .map(|p| p as u32)
        .collect())
}

#[cfg(target_os = "macos")]
fn bsd_info(pid: u32) -> io::Result<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if count != size as i32 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.assume_init() })
}

#[cfg(target_os = "macos")]
fn mac_environment(pid: u32) -> io::Result<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    // Arguments plus environment can reach kern.argmax; a smaller buffer fails with ENOMEM.
    let mut argmax: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let argmax_ok = unsafe {
        libc::sysctlbyname(
            c"kern.argmax".as_ptr(),
            (&mut argmax as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } == 0;
    let capacity = if argmax_ok && argmax > 0 {
        argmax as usize
    } else {
        1024 * 1024
    };
    let mut buffer = vec![0u8; capacity];
    let mut len = buffer.len();
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(len);
    if len < 4 {
        return Err(io::Error::other("incomplete process arguments"));
    }
    let argc = i32::from_ne_bytes(buffer[..4].try_into().unwrap());
    if argc < 0 {
        return Err(io::Error::other("invalid argc"));
    }
    let mut pos = 4;
    while pos < len && buffer[pos] != 0 {
        pos += 1;
    }
    while pos < len && buffer[pos] == 0 {
        pos += 1;
    }
    for _ in 0..argc {
        while pos < len && buffer[pos] != 0 {
            pos += 1;
        }
        if pos < len {
            pos += 1;
        }
    }
    Ok(buffer[pos..].to_vec())
}

#[cfg(target_os = "macos")]
fn inspect_native(backend: &NativeBackend, pid: u32) -> Inspection {
    let info = match bsd_info(pid) {
        Ok(info) => info,
        Err(e) if e.raw_os_error() == Some(libc::ESRCH) => return Inspection::Gone,
        Err(e) => return Inspection::Unavailable(e.to_string()),
    };
    let mut path = [0u8; 4096];
    let path_size =
        unsafe { libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32) };
    let executable = if path_size > 0 {
        Some(
            String::from_utf8_lossy(path.split(|&b| b == 0).next().unwrap_or_default())
                .into_owned(),
        )
    } else {
        None
    };
    let environment = if info.pbi_uid == backend.uid {
        mac_environment(pid).ok()
    } else {
        None
    };
    match bsd_info(pid) {
        Ok(after)
            if (after.pbi_start_tvsec, after.pbi_start_tvusec, after.pbi_uid)
                == (info.pbi_start_tvsec, info.pbi_start_tvusec, info.pbi_uid) => {}
        _ => return Inspection::Unavailable("process changed during inspection".into()),
    }
    let name_bytes: Vec<_> = info
        .pbi_name
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    Inspection::Present(Process {
        identity: ProcessIdentity {
            host: backend.host.clone(),
            boot: backend.boot.clone(),
            uid: info.pbi_uid,
            pid,
            start: format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        },
        parent_pid: info.pbi_ppid,
        executable,
        name: String::from_utf8_lossy(&name_bytes).into_owned(),
        zombie: info.pbi_status == 5,
        session_tag: environment.as_deref().and_then(session_tag),
        tag_readable: environment.is_some(),
        manager_owned: None,
    })
}

#[cfg(target_os = "macos")]
fn launchd_pids() -> Option<BTreeSet<u32>> {
    let output = Command::new("/bin/launchctl").arg("list").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .collect(),
    )
}

#[cfg(target_os = "macos")]
fn listening_ports(_: &BTreeMap<u32, Process>) -> Result<Vec<Port>> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .output()?;
    if (!output.status.success() && output.status.code() != Some(1)) || !output.stderr.is_empty() {
        return Err(format!("lsof: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    parse_lsof(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_lsof(text: &str) -> Result<Vec<Port>> {
    let mut pid = None;
    let mut ports = Vec::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse::<u32>().ok();
        }
        if let Some(address) = line.strip_prefix('n') {
            if let (Some(pid), Some((host, port))) = (pid, address.rsplit_once(':')) {
                if let Ok(port) = port.parse::<u16>() {
                    ports.push(Port {
                        pid,
                        address: host.trim_matches(['[', ']']).into(),
                        port,
                    });
                }
            }
        }
    }
    ports.sort_by_key(|p| (p.pid, p.port, p.address.clone()));
    ports.dedup();
    Ok(ports)
}
