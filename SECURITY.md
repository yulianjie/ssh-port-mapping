# Security policy

## Reporting a vulnerability

Please do not open a public issue for a vulnerability that could expose credentials or make a
forwarded service reachable by unintended users. Report it privately through GitHub Security
Advisories for this repository.

Include the PortWeave version, operating system, OpenSSH version, reproduction steps, and the
least-sensitive log excerpt that demonstrates the problem. Never attach private keys, passwords,
agent data, or a complete personal SSH configuration.

## Security model

PortWeave delegates authentication, host-key checking, and transport security to the system
OpenSSH client. It stores tunnel metadata and an optional identity-file path, but never stores
passwords, key contents, or agent credentials. Users remain responsible for SSH server policy,
firewall rules, `GatewayPorts`, and the permissions of local key and configuration files.
