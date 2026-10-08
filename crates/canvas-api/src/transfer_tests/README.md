# Loopback TLS fixture

`cert.pem` and `key.pem` are a public test certificate and its private key.
`tls.rs` includes them only in the test harness and binds the server to
`127.0.0.1` on an ephemeral port. The test client trusts that certificate
explicitly. They are not production authentication material.

Anyone with this repository can use the key. Never reuse it for a deployed
service, a Canvas account, or another trust store. A secret scanner may correctly
identify the PEM block as a private key. That alert is expected for this one
fixture and should not justify ignoring unrelated private-key alerts.
