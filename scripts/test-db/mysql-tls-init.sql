CREATE USER 'certuser'@'%' REQUIRE X509;
GRANT SELECT ON spike.* TO 'certuser'@'%';
