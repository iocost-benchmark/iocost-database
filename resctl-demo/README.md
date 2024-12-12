<img src="img/logo.svg" alt="resctl-demo logo" width="50%"/>

Resource control aims to control compute resource distribution to improve
reliability and utilization of a system. The facebook kernel and container
teams have been intensively researching and implementing mechanisms and
methods to advance resource control. This repository contains two projects -
resctl-demo and resctl-bench.

resctl-demo
-----------

resctl-demo demonstrates and documents various aspects of resource control
using self-contained workloads in guided scenarios.

<a href="https://engineering.fb.com/wp-content/uploads/2020/10/resctl-demoV2.mp4">
  <img src="img/screenshot.png" alt="resctl-demo in action" width="50%">
</a>

resctl-bench
------------

resctl-bench is a collection of whole-system benchmarks to evaluate resource
control and hardware behaviors using realistic simulated workloads.

Comprehensive resource control involves the whole system. Furthermore,
testing resource control end-to-end requires scenarios involving realistic
workloads and monitoring their interactions. The combination makes
benchmarking resource control challenging and error-prone. It's easy to slip
up on a configuration and testing with real workloads can be tedious and
unreliable.

resctl-bench encapsulates the whole process so that resource control
benchmarks can be performed easily and reliably. It verifies and updates
system configurations, reproduces resource contention scenarios with a
realistic latency-sensitive workload simulator and other secondary
workloads, analyzes the resulting system and workload behaviors, and
generates easily understandable reports.

Read the [documentation](resctl-bench/README.md) for more information.


Premade System Images
=====================

Comprehensive resource control has many requirements, some of which can be
difficult to configure on an existing system. resctl-demo provides premade
images to help getting started. Visit the following page for details:

  https://facebookmicrosites.github.io/resctl-demo-website

Installation
============

`resctl-demo` and `resctl-bench` are packaged in Fedora as of Fedora 34 and
in EPEL as of EPEL 8. They can be installed with:

```
sudo dnf install resctl-demo resctl-bench
```

which will pull in any other dependencies that might be required. On Fedora,
you will also want to disable zram based swap:

```
touch /etc/systemd/zram-generator.conf
systemctl stop dev-zram0.swap
```

For other distributions, please follow the next sections to install from
cargo or from source.

Installation with cargo
=======================

resctl-demo and resctl-bench can be installed using cargo which is the
package manager for rust. cargo can be installed with rustup:

  https://rustup.rs/

For distro-specific way to install cargo, see the distro sub-sections. Note
that the distro packaged version might not be recent enough.

Once cargo is available, run the following command to install resctl-bench
and resctl-demo. Don't forget to install rd-hashd and rd-agent.

```
cargo install rd-hashd rd-agent resctl-demo resctl-bench
```

cargo installs under `$HOME/.cargo/bin` by default. Feel free to copy them
elsewhere as convenient. For example:

```
cd ~/.cargo/bin
cp rd-hashd rd-agent resctl-demo resctl-bench /usr/local/bin
```

Information on installing cargo and other dependencies on different distros
follows.


Arch
----

Installing cargo:

```
pacman -S --needed rust
```

The common dependencies:

```
pacman -S --needed coreutils util-linux python python-bcc fio stress
```

oomd is available through AUR:

```
git clone https://aur.archlinux.org/oomd-git.git oomd-git
cd oomd-git
makepkg -si
```

resctl-demo needs the followings to plot graphs and run linux build job as
one of the workloads:

```
pacman -S --needed gnuplot gcc binutils make bison flex pkgconf openssl libelf
```


Fedora
------

Installing cargo:

```
dnf install cargo
```

The common dependencies:

```
dnf install coreutils util-linux python3 python3-bcc fio stress oomd
```

resctl-demo needs the followings to plot graphs and run linux build job as
one of the workloads:

```
dnf install gnuplot gcc binutils make bison flex pkgconf openssl-devel elfutils-devel
```

Disable zram based swap:

```
touch /etc/systemd/zram-generator.conf
systemctl stop dev-zram0.swap
```

If `journalctl -u rd-agent` shows EXEC failures, put SELinux in permissive mode
by setting `SELINUX=permissive` in `/etc/selinux/config` and rebooting.


Ubuntu
------

Installing cargo:

```
apt install cargo
```

The common dependencies:

```
apt install coreutils util-linux python3 python3-bpfcc fio stress oomd
```

resctl-demo needs the followings to plot graphs and run linux build job as
one of the workloads:

```
apt install gnuplot gcc binutils make bison flex pkgconf libssl-dev libelf-dev
```


Building and Installing Manually
================================

Building is straight-forward. Check out the source code and run:

```
cargo build --release
```

Installing from local source directory:

```
cargo install --path rd-hashd
cargo install --path rd-agent
cargo install --path resctl-demo
cargo install --path resctl-bench
```

Alternatively, run `build-and-tar.sh` script to create a tarball containing
the binaries:

```
./build-and-tar.sh
```

You can install resctl-demo and resctl-bench by simply untarring the
resulting tarball:

```
cd /usr/local/bin
tar xvzf $SRC_DIR/target/resctl-demo.tar.gz
```

Follow the instructions in the Installation section to install other
dependencies.


Running resctl-demo
===================

resctl-demo should be run as root in hostcritical.slice. Use the following
command:

```
sudo systemd-run --scope --slice hostcritical.slice --unit resctl-demo /usr/local/bin/resctl-demo
```


Requirements
============

The basic building blocks are provided by the Linux kernel's cgroup2 and other
resource related features. On top, usage and configuration methods combined with
user-space helpers such as oomd and sideloader implement resource isolation to
achieve workload protection and stacking.

* Linux kernel in the git branch
  `https://git.kernel.org/pub/scm/linux/kernel/git/tj/misc.git
  resctl-demo-v5.13-rc7` which contains the following extra commits on top
  of v5.13-rc7:
    * Four mm commits to [fix inode shadow entry
      protection](resctl-bench/doc/shadow-inode.md)
    * Backport of [`blkcg: drop CLONE_IO check in
      blkcg_can_attach()`](https://git.kernel.org/pub/scm/linux/kernel/git/axboe/linux-block.git/commit/?h=for-5.14/block&id=b5f3352e0868611b555e1dcb2e1ffb8e346c519c)
* cgroup2
* btrfs on non-composite storage device (sda or nvme0n1, not md or dm)
* Swap file on btrfs at least as large as 1/3 of physical memory
* systemd
* oomd
* dd, stdbuf, findmnt, python3, fio, stress, gnuplot, gcc, ld, make, bison,
  flex, pkg-config, libssl, libelf


License
=======

resctl-demo is apache-2.0 licensed, as found in the [LICENSE](LICENSE) file.
