This program copies the UDF files from a DVD-ROM disc into a VIDEO_TS directory.
It is basically like [`dvdbackup`](https://dvdbackup.sourceforge.net/)
and currently there is no particular reason to use it.

## Usage

```
Usage: dvdromcopy [OPTIONS] --device <DEVICE> --output <OUTPUT>

Options:
  -d, --device <DEVICE>    The DVD device or file to open
  -o, --output <OUTPUT>    The output directory to write the DVD to
      --name <NAME>        Name of the DVD; if not specified then it will read from DVD primary_volume.volume_identifier
      --include <INCLUDE>  Include only the specified files and directories
  -h, --help               Print help
  -V, --version            Print version
```

Example: on MacOS, the DVD drive is usually /dev/rdisk4 so I run the program like this:

```
dvdromcopy --device /dev/rdisk4 --output ~/Movies
```

This will read the DVD name from the volume identifier (e.g. FUNFANCY),
turn it into titlecase (e.g. Funfancy), and create directories and files
e.g. `~/Movies/Funfancy/VIDEO_TS/VIDEO_TS.IFO`, `VIDEO_TS.VOB`, etc.

To enable debugging, you can add `RUST_BACKTRACE` and `RUST_LOG`:

```
RUST_BACKTRACE=1 RUST_LOG=main=debug,dvdbackup2=debug cargo run -- --device /dev/rdisk4 --output ~/Movies
```

## Dependencies

This is a rust program. First, you need to install rust and cargo
e.g. https://rustup.rs/ or [rust-overlay](https://github.com/oxalica/rust-overlay)
if you use nix.

In addition to static dependencies that `cargo` automatically installs,
this program depends on `libdvdcss` to decrypt DVD blocks.

If you use nix and direnv, you can set them up with this `.envrc` file:

```
# .envrc
use nix -p libdvdcss -p pkg-config -p xcbuild
```

Or install it using whichever package manager you normally use
e.g. `brew install libdvdcss`, `sudo port install libdvdcss`,
`apt-get install libdvdcss-dev`.

Then you build it using

```
cargo build --profile=release
```

This will compile the executable binary `target/release/dvdromcopy`.