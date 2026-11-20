import base64
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile

ks_path = Path(".local/android/debug.keystore")
ks_path.parent.mkdir(parents=True, exist_ok=True)
ks_path.unlink(missing_ok=True)
keytool = Path(os.environ["JAVA_HOME"]) / "bin/keytool"

raw_b64 = os.environ.get("ANDROID_KEYSTORE_BASE64", "").strip()
if raw_b64:
    ks_path.write_bytes(base64.b64decode(raw_b64))
else:
    SMALL_PRIMES = [
        p for p in range(3, 2000, 2)
        if all(p % d for d in range(3, int(p ** 0.5) + 1, 2))
    ]

    def is_probable_prime(n: int) -> bool:
        for p in SMALL_PRIMES:
            if n % p == 0:
                return n == p
        d = n - 1
        r = (d & -d).bit_length() - 1
        d >>= r
        for a in (2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37):
            x = pow(a, d, n)
            if x == 1 or x == n - 1:
                continue
            for _ in range(r - 1):
                x = pow(x, 2, n)
                if x == n - 1:
                    break
            else:
                return False
        return True

    def derive_prime(seed: bytes, bits: int = 1024, e: int = 65537) -> int:
        counter = 0
        blocks = bits // 256
        while True:
            buf = b"".join(
                hashlib.sha256(seed + counter.to_bytes(4, "big") + i.to_bytes(2, "big")).digest()
                for i in range(blocks)
            )
            counter += 1
            cand = int.from_bytes(buf, "big")
            cand |= (1 << (bits - 1)) | (1 << (bits - 2)) | 1
            if (cand - 1) % e != 0 and is_probable_prime(cand):
                return cand

    def der_len(n: int) -> bytes:
        if n < 0x80:
            return bytes([n])
        b = n.to_bytes((n.bit_length() + 7) // 8, "big")
        return bytes([0x80 | len(b)]) + b

    def der_tlv(tag: int, body: bytes) -> bytes:
        return bytes([tag]) + der_len(len(body)) + body

    def der_int(v: int) -> bytes:
        b = v.to_bytes(max(1, (v.bit_length() + 7) // 8), "big")
        if b[0] & 0x80:
            b = b"\x00" + b
        return der_tlv(0x02, b)

    def der_seq(*items: bytes) -> bytes:
        return der_tlv(0x30, b"".join(items))

    def der_oid(oid: bytes) -> bytes:
        return der_tlv(0x06, oid)

    def der_bitstr(b: bytes) -> bytes:
        return der_tlv(0x03, b"\x00" + b)

    repo = os.environ.get("SIGNING_REPO", "entitybrian69-bit/Volt-launcher")
    e = 65537
    p = derive_prime(f"volt-rsa-p-v1:{repo}".encode(), 1024, e)
    q = derive_prime(f"volt-rsa-q-v1:{repo}".encode(), 1024, e)
    if p < q:
        p, q = q, p
    n = p * q
    phi = (p - 1) * (q - 1)
    d = pow(e, -1, phi)
    dp = d % (p - 1)
    dq = d % (q - 1)
    qi = pow(q, -1, p)

    rsa_priv = der_seq(
        der_int(0), der_int(n), der_int(e), der_int(d),
        der_int(p), der_int(q), der_int(dp), der_int(dq), der_int(qi),
    )
    rsa_oid = der_seq(der_oid(bytes.fromhex("2a864886f70d010101")), b"\x05\x00")
    pkcs8_der = der_seq(der_int(0), rsa_oid, der_tlv(0x04, rsa_priv))
    spki = der_seq(rsa_oid, der_bitstr(der_seq(der_int(n), der_int(e))))

    sha256_rsa = der_seq(der_oid(bytes.fromhex("2a864886f70d01010b")), b"\x05\x00")
    name = der_seq(der_tlv(0x31, der_seq(
        der_oid(bytes.fromhex("550403")),
        der_tlv(0x0c, b"Volt Launcher"),
    )))
    validity = der_seq(
        der_tlv(0x17, b"250101000000Z"),
        der_tlv(0x18, b"20550101000000Z"),
    )
    tbs = der_seq(
        der_tlv(0xa0, der_int(2)),
        der_int(1),
        sha256_rsa,
        name,
        validity,
        name,
        spki,
    )
    digest_info = bytes.fromhex("3031300d060960864801650304020105000420") + hashlib.sha256(tbs).digest()
    em = b"\x00\x01" + (b"\xff" * (256 - 3 - len(digest_info))) + b"\x00" + digest_info
    sig_bytes = pow(int.from_bytes(em, "big"), d, n).to_bytes(256, "big")
    cert_der = der_seq(tbs, sha256_rsa, der_bitstr(sig_bytes))

    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        (t / "key.der").write_bytes(pkcs8_der)
        (t / "cert.der").write_bytes(cert_der)
        subprocess.run(["openssl", "pkey", "-inform", "DER", "-in", str(t / "key.der"), "-out", str(t / "key.pem")], check=True)
        subprocess.run(["openssl", "x509", "-inform", "DER", "-in", str(t / "cert.der"), "-out", str(t / "cert.pem")], check=True)
        subprocess.run([
            "openssl", "pkcs12", "-export",
            "-inkey", str(t / "key.pem"),
            "-in", str(t / "cert.pem"),
            "-name", "androiddebugkey",
            "-passout", "pass:android",
            "-out", str(t / "stage.p12"),
        ], check=True)
        subprocess.run([
            str(keytool), "-importkeystore",
            "-srckeystore", str(t / "stage.p12"),
            "-srcstoretype", "PKCS12",
            "-srcstorepass", "android",
            "-destkeystore", str(ks_path),
            "-deststoretype", "PKCS12",
            "-deststorepass", "android",
            "-noprompt",
        ], check=True)

    print("Persistent signing certificate SHA-256:", hashlib.sha256(cert_der).hexdigest(), flush=True)

subprocess.run([
    str(keytool), "-list",
    "-keystore", str(ks_path),
    "-storepass", "android",
    "-alias", "androiddebugkey",
], check=True)
