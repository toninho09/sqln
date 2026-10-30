#!/bin/sh
# Creates throwaway test certificates in .test-certs/. Do not use them outside tests.
set -eu

cd "$(dirname "$0")/.."
rm -rf .test-certs
mkdir -p .test-certs
cd .test-certs

openssl req -x509 -newkey rsa:2048 -nodes -days 30 -keyout ca.key -out ca.crt -subj '/CN=sqln-test-ca' 2>/dev/null
openssl req -x509 -newkey rsa:2048 -nodes -days 30 -keyout other-ca.key -out other-ca.crt -subj '/CN=sqln-other-ca' 2>/dev/null

openssl req -newkey rsa:2048 -nodes -keyout server.key -out server.csr -subj '/CN=localhost' 2>/dev/null
printf 'subjectAltName=DNS:localhost\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' > server.ext
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 30 -out server.crt -extfile server.ext 2>/dev/null

openssl req -newkey rsa:2048 -nodes -keyout client.key -out client.csr -subj '/CN=certuser' 2>/dev/null
printf 'basicConstraints=CA:FALSE\nextendedKeyUsage=clientAuth\n' > client.ext
openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 30 -out client.crt -extfile client.ext 2>/dev/null

# tokio-postgres reads PKCS#8. mysql_async reads RSA PKCS#1 only.
openssl pkcs8 -topk8 -nocrypt -in client.key -out client.pk8.key 2>/dev/null
openssl rsa -traditional -in client.key -out client.rsa.key 2>/dev/null ||
    openssl rsa -in client.key -out client.rsa.key 2>/dev/null

cat > pg_hba.conf <<'HBA'
local all all trust
hostssl all certuser all cert
host all all all scram-sha-256
HBA

chmod 644 ./*.key ./*.crt pg_hba.conf
