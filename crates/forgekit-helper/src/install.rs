//! `forgekit-helper --install [--root DIR]` and `--uninstall`.
//!
//! With the default root ("/") this needs to run as root (the app uses
//! `pkexec`) and also enables the service. With `--root` it only lays out the
//! files, for distro packages.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

pub const BIN: &str = "usr/libexec/forgekit-helper";
const UNIT: &str = "etc/systemd/system/forgekit-helper.service";
const DBUS_POLICY: &str = "usr/share/dbus-1/system.d/org.forgekit.Helper.conf";
const POLKIT: &str = "usr/share/polkit-1/actions/org.forgekit.helper.policy";
const MODULES: &str = "etc/modules-load.d/forgekit-uinput.conf";

pub const UNIT_FILE: &str = "[Unit]
Description=Forgekit input helper (Keysmith key remapping, Umlaut accents)
Documentation=https://github.com/anuzdhk/forgekit

[Service]
Type=dbus
BusName=org.forgekit.Helper
ExecStart=/usr/libexec/forgekit-helper
Restart=on-failure
StateDirectory=forgekit-helper
StateDirectoryMode=0700
# Only keyboards and uinput; no network, no capabilities, read-only system.
DevicePolicy=closed
DeviceAllow=char-input rw
DeviceAllow=/dev/uinput rw
CapabilityBoundingSet=
NoNewPrivileges=yes
PrivateNetwork=yes
RestrictAddressFamilies=AF_UNIX
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
SystemCallArchitectures=native
SystemCallFilter=@system-service
UMask=0077

[Install]
WantedBy=multi-user.target
";

pub const DBUS_POLICY_FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <policy user="root">
    <allow own="org.forgekit.Helper"/>
  </policy>
  <!-- Anyone may ask; changes are checked with polkit inside the helper. -->
  <policy context="default">
    <allow send_destination="org.forgekit.Helper"/>
  </policy>
</busconfig>
"#;

pub const POLKIT_FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <vendor>Forgekit</vendor>
  <vendor_url>https://github.com/anuzdhk/forgekit</vendor_url>
  <action id="org.forgekit.helper.configure">
    <description>Change keyboard remapping</description>
    <message>Authentication is required to change how your keyboard is remapped</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>auth_admin_keep</allow_active>
    </defaults>
  </action>
</policyconfig>
"#;

fn files() -> [(&'static str, &'static str, u32); 4] {
    [
        (UNIT, UNIT_FILE, 0o644),
        (DBUS_POLICY, DBUS_POLICY_FILE, 0o644),
        (POLKIT, POLKIT_FILE, 0o644),
        (MODULES, "uinput\n", 0o644),
    ]
}

fn write(path: &Path, contents: &[u8], mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("forgekit-tmp");
    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, path).with_context(|| format!("installing {}", path.display()))
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

pub fn install(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let live = root == Path::new("/");
    if live && !is_root() {
        bail!("run this as root (the Forgekit app does it with pkexec)");
    }
    let exe = std::fs::read(std::env::current_exe()?).context("reading this program")?;
    let mut written = vec![root.join(BIN)];
    write(&root.join(BIN), &exe, 0o755)?;
    for (rel, contents, mode) in files() {
        write(&root.join(rel), contents.as_bytes(), mode)?;
        written.push(root.join(rel));
    }
    if live {
        run("systemctl", &["daemon-reload"])?;
        let _ = run("modprobe", &["uinput"]);
        run("systemctl", &["enable", "--now", "forgekit-helper.service"])?;
        // Pick up the new bus policy before clients call us.
        let _ = run("systemctl", &["reload", "dbus.service"]);
    }
    Ok(written)
}

pub fn uninstall(root: &Path) -> anyhow::Result<()> {
    let live = root == Path::new("/");
    if live && !is_root() {
        bail!("run this as root (the Forgekit app does it with pkexec)");
    }
    if live {
        let _ = run("systemctl", &["disable", "--now", "forgekit-helper.service"]);
    }
    for (rel, _, _) in files() {
        let _ = std::fs::remove_file(root.join(rel));
    }
    let _ = std::fs::remove_dir_all(root.join("var/lib/forgekit-helper"));
    if live {
        let _ = run("systemctl", &["daemon-reload"]);
    }
    // Last, since this may be the running program.
    let _ = std::fs::remove_file(root.join(BIN));
    Ok(())
}

fn run(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let st = Command::new(cmd).args(args).status().with_context(|| format!("running {cmd}"))?;
    if !st.success() {
        bail!("{cmd} {} failed", args.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_into_a_staging_root() {
        let d = tempfile::tempdir().unwrap();
        let written = install(d.path()).unwrap();
        assert_eq!(written.len(), 5);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.path().join(BIN)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let unit = std::fs::read_to_string(d.path().join(UNIT)).unwrap();
        assert!(unit.contains("ExecStart=/usr/libexec/forgekit-helper") && unit.contains("PrivateNetwork=yes"));
        uninstall(d.path()).unwrap();
        assert!(!d.path().join(BIN).exists() && !d.path().join(POLKIT).exists());
    }
}
