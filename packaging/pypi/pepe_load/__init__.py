"""pepe from PyPI.

pepe is a Rust binary; this package is the `pepe` command that fetches it.
The first run downloads the release archive of this package's version for
the running platform from GitHub, checks it against the SHA-256 the release
carries, keeps the binary in a per-user cache directory, and runs it with
the arguments given; every later run only runs it. Nothing but the standard
library is used.

    PEPE_LOAD_CACHE_DIR   where the binary is kept (default: the user's
                          cache directory, under pepe-load/)
"""

import hashlib
import io
import os
import platform
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile

# pepe's version: the release this package downloads. The publish job
# (.github/workflows/publish-pypi.yml) sets it from the release tag.
__version__ = "0.33.0"

REPO = "omarmhaimdat/pepe"
RELEASE_URL = "https://github.com/" + REPO + "/releases/download/v{version}/{asset}"
INSTALL_DOCS = "https://pepe.mhaimdat.com/docs/install.html"


class Problem(Exception):
    """Something this package could not do; the message says what."""


def target():
    """The release target for this machine: (triple, archive extension)."""
    system = platform.system()
    machine = platform.machine().lower()
    arm = machine in ("arm64", "aarch64")
    x86 = machine in ("x86_64", "amd64")
    if system == "Darwin" and arm:
        return "aarch64-apple-darwin", "tar.xz"
    if system == "Darwin" and x86:
        return "x86_64-apple-darwin", "tar.xz"
    if system == "Linux" and x86:
        return "x86_64-unknown-linux-musl", "tar.xz"
    if system == "Linux" and arm:
        return "aarch64-unknown-linux-musl", "tar.xz"
    if system == "Windows" and (x86 or arm):
        # Windows on ARM runs the x86_64 build through its emulation
        return "x86_64-pc-windows-msvc", "zip"
    raise Problem(
        "there is no pepe build for %s %s; the other ways to install are at %s"
        % (system, machine, INSTALL_DOCS)
    )


def cache_dir():
    """Where the binary is kept: the user's cache directory, as the platform
    has it, unless PEPE_LOAD_CACHE_DIR says otherwise."""
    override = os.environ.get("PEPE_LOAD_CACHE_DIR")
    if override:
        return override
    home = os.path.expanduser("~")
    if sys.platform == "win32":
        base = os.environ.get("LOCALAPPDATA") or os.path.join(home, "AppData", "Local")
        return os.path.join(base, "pepe-load", "Cache")
    if sys.platform == "darwin":
        return os.path.join(home, "Library", "Caches", "pepe-load")
    base = os.environ.get("XDG_CACHE_HOME") or os.path.join(home, ".cache")
    return os.path.join(base, "pepe-load")


def binary_path():
    """The binary of this version for this machine, in the cache (it may not
    be there yet)."""
    triple, _ = target()
    name = "pepe.exe" if sys.platform == "win32" else "pepe"
    return os.path.join(cache_dir(), "v" + __version__, triple, name)


def fetch(url):
    """The bytes at a URL, or a Problem that says why not."""
    request = urllib.request.Request(url, headers={"User-Agent": "pepe-load/" + __version__})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        if error.code == 404:
            raise Problem(
                "release v%s has no %s; the releases are at https://github.com/%s/releases"
                % (__version__, url.rsplit("/", 1)[-1], REPO)
            )
        raise Problem("could not download %s: HTTP %s" % (url, error.code))
    except (urllib.error.URLError, OSError) as error:
        raise Problem("could not download %s: %s" % (url, error.reason if hasattr(error, "reason") else error))


def verify(archive, checksum_file, asset):
    """The archive's SHA-256 must be the one the release says."""
    expected = checksum_file.decode("utf-8", "replace").split()
    if not expected or len(expected[0]) != 64:
        raise Problem("the checksum file for %s is not one: %r" % (asset, checksum_file[:80]))
    actual = hashlib.sha256(archive).hexdigest()
    if actual != expected[0].lower():
        raise Problem(
            "%s does not match its checksum (expected %s, got %s); not installing it"
            % (asset, expected[0], actual)
        )


def binary_from(archive, extension, name):
    """The bytes of the binary inside a release archive. Only that member is
    read; the archive's own paths are not used for anything."""
    if extension == "zip":
        with zipfile.ZipFile(io.BytesIO(archive)) as zf:
            for member in zf.infolist():
                if member.filename.rsplit("/", 1)[-1] == name and not member.is_dir():
                    return zf.read(member)
    else:
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:xz") as tf:
            for member in tf:
                if member.isfile() and member.name.rsplit("/", 1)[-1] == name:
                    return tf.extractfile(member).read()
    raise Problem("the release archive has no %s in it" % name)


def download(path):
    """Download, verify and keep this version's binary at `path`."""
    triple, extension = target()
    asset = "pepe-%s.%s" % (triple, extension)
    url = RELEASE_URL.format(version=__version__, asset=asset)
    sys.stderr.write("pepe-load: downloading pepe v%s for %s\n" % (__version__, triple))
    sys.stderr.flush()
    archive = fetch(url)
    verify(archive, fetch(url + ".sha256"), asset)
    binary = binary_from(archive, extension, os.path.basename(path))

    # Written beside its place and moved in, so a run that starts meanwhile
    # sees either nothing or the whole binary
    directory = os.path.dirname(path)
    os.makedirs(directory, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".pepe-", dir=directory)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(binary)
        os.chmod(temporary, 0o755)
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise


def ensure():
    """The path of the binary, downloaded if it isn't there."""
    path = binary_path()
    if not os.path.isfile(path):
        download(path)
    return path


def main(argv=None):
    """Run pepe with the arguments given (the process's own by default).
    Returns pepe's exit code, or 1 when the binary could not be had."""
    args = list(sys.argv[1:] if argv is None else argv)
    try:
        exe = ensure()
    except Problem as problem:
        sys.stderr.write("pepe-load: %s\n" % problem)
        return 1
    if os.name == "posix":
        os.execv(exe, [exe] + args)
    # Windows has no exec: run it and pass its exit code on. Ctrl+C reaches
    # both processes; let pepe finish its own handling of it
    process = subprocess.Popen([exe] + args)
    try:
        return process.wait()
    except KeyboardInterrupt:
        return process.wait()
