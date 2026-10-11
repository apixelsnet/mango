#!/usr/bin/env python3
"""Rebuilds the signed test jars in this folder. Needs the openssl command.

    python3 make.py

signed.jar is signed the way Android signs an extension: the signature is over the
.SF file itself. signed-attrs.jar is signed the way jarsigner can: over a set of
signed attributes. The key is made fresh and thrown away, so a run replaces all
three files together.
"""
import base64
import hashlib
import pathlib
import subprocess
import tempfile
import zipfile

here = pathlib.Path(__file__).parent
files = {
    "AndroidManifest.xml": b"<manifest package='example.test'/>\n",
    "example/Main.class": b"\xca\xfe\xba\xbe not a real class\n",
    # Long enough that its manifest line has to be folded.
    "res/" + "long-name-" * 8 + "icon.png": b"\x89PNG\r\n",
}


def digest(data):
    return base64.b64encode(hashlib.sha256(data).digest()).decode()


def line(text):
    """A manifest line: at most 72 bytes, the rest continued after a space."""
    raw = text.encode()
    out, raw = raw[:72], raw[72:]
    while raw:
        out, raw = out + b"\r\n " + raw[:71], raw[71:]
    return out + b"\r\n"


manifest = line("Manifest-Version: 1.0") + b"\r\n"
signature_file = b""
for name, data in files.items():
    section = line(f"Name: {name}") + line(f"SHA-256-Digest: {digest(data)}") + b"\r\n"
    manifest += section
    signature_file += line(f"Name: {name}") + line(f"SHA-256-Digest: {digest(section)}") + b"\r\n"
signature_file = (
    line("Signature-Version: 1.0") + line(f"SHA-256-Digest-Manifest: {digest(manifest)}") + b"\r\n" + signature_file
)

with tempfile.TemporaryDirectory() as tmp:
    tmp = pathlib.Path(tmp)

    def openssl(*args):
        return subprocess.run(("openssl",) + args, cwd=tmp, check=True, capture_output=True).stdout

    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "36500", "-subj", "/CN=Mango test",
            "-keyout", "key.pem", "-out", "cert.pem")
    (tmp / "CERT.SF").write_bytes(signature_file)
    for jar, style in (("signed.jar", ["-noattr"]), ("signed-attrs.jar", [])):
        openssl("smime", "-sign", "-binary", "-md", "sha256", *style, "-in", "CERT.SF", "-signer", "cert.pem",
                "-inkey", "key.pem", "-outform", "DER", "-out", "CERT.RSA")
        with zipfile.ZipFile(here / jar, "w", zipfile.ZIP_DEFLATED) as archive:
            for name, data in files.items():
                archive.writestr(name, data)
            archive.writestr("META-INF/MANIFEST.MF", manifest)
            archive.writestr("META-INF/CERT.SF", signature_file)
            archive.writestr("META-INF/CERT.RSA", (tmp / "CERT.RSA").read_bytes())
    certificate = openssl("x509", "-in", "cert.pem", "-outform", "DER")
    (here / "fingerprint.txt").write_text(hashlib.sha256(certificate).hexdigest() + "\n")

print("wrote signed.jar, signed-attrs.jar and fingerprint.txt")
