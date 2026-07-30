# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| A-2.0.0 | Yes |
| A-1.0.0 | Security fixes only while A-2.x is current |

Kroa is Alpha software. Treat it as unsuitable for production workloads that require strong security guarantees.

## Reporting a vulnerability

Please report security issues privately through GitHub Security Advisories for this repository, or by contacting the repository owner.

Include:

1. Kroa version / commit hash
2. Affected platform
3. Minimal reproduction steps
4. Impact assessment

Do not open a public issue for vulnerabilities that could enable remote code execution, memory corruption outside intentional `unsafe` boundaries, or supply-chain compromise.

## Scope notes

- The compiler itself runs with the privileges of the developer machine.
- Generated programs may include runtime abort paths for bounds checks.
- `unsafe` and `extern "C"` boundaries are intentionally trusted; misuse is undefined.
