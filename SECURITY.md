# Security Policy

## Supported Versions

We release patches for security vulnerabilities for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 2026.x  | :white_check_mark: |
| < 2026  | :x:                |

We use Calendar Versioning (CalVer) with the format `YYYY.M.D`. Only the current year's releases receive security updates.

## Reporting a Vulnerability

We take the security of gh0st seriously. If you believe you have found a security vulnerability, please report it to us as described below.

### Please Do NOT:

- Open a public GitHub issue for security vulnerabilities
- Disclose the vulnerability publicly before it has been addressed

### Please DO:

1. **Email us directly** at security@example.com (replace with your actual security contact)
2. **Provide detailed information** including:
   - Description of the vulnerability
   - Steps to reproduce the issue
   - Potential impact
   - Affected versions
   - Suggested fix (if any)

### What to Expect:

- **Initial Response**: We will acknowledge receipt of your vulnerability report within 48 hours
- **Status Updates**: We will keep you informed of our progress as we work on a fix
- **Resolution Timeline**: We aim to resolve critical vulnerabilities within 7 days
- **Credit**: We will credit you in the security advisory (unless you prefer to remain anonymous)

## Security Best Practices for Users

### Running gh0st Safely

1. **Use the Latest Version**: Always run the most recent release to ensure you have the latest security patches

2. **Validate Input URLs**: Be cautious when crawling untrusted domains

   ```bash
   # Good: Crawl known, trusted domains
   gh0st https://example.com

   # Caution: Review URLs from untrusted sources
   ```

3. **Limit Crawl Scope**: Use depth and domain restrictions to prevent unintended crawling

   ```bash
   gh0st https://example.com --depth 5 --no-subdomains
   ```

4. **WebDriver Security**: When using WebDriver features:
   - Use `--webdriver-allowed-ips` to restrict access
   - Don't expose WebDriver endpoints to untrusted networks
   - Use `--webdriver-headless` in production environments

5. **File Permissions**: Ensure output files have appropriate permissions

   ```bash
   # Set restrictive permissions on output files
   chmod 600 results.csv
   ```

6. **Container Security**: When using Docker:
   - Run containers with read-only root filesystem where possible
   - Use resource limits to prevent resource exhaustion
   - Keep Docker images updated

### Data Handling

1. **Sensitive Data**: Be aware that crawled data may contain sensitive information
   - Review and sanitize output before sharing
   - Use encryption for stored crawl results if they contain sensitive data

2. **API Keys and Credentials**: Never include API keys or credentials in crawl configurations
   - Use environment variables for sensitive configuration
   - Don't commit configuration files with secrets to version control

3. **robots.txt Compliance**: Use `--respect-robots` to honor site policies
   ```bash
   gh0st https://example.com --respect-robots
   ```

## Known Security Considerations

### WebDriver Mode

When using WebDriver mode (`--webdriver`), be aware that:

- A browser instance will be launched which can consume significant resources
- The WebDriver endpoint, if exposed, could be used to execute arbitrary browser actions
- Downloaded browser binaries are cached locally - ensure your system is secure

**Mitigation**: Use `--webdriver-allowed-ips` and firewall rules to restrict access.

### Network Requests

gh0st makes HTTP/HTTPS requests to target domains:

- DNS rebinding attacks could potentially redirect requests
- SSRF (Server-Side Request Forgery) risks if crawling user-supplied URLs

**Mitigation**: Validate and sanitize input URLs, use network isolation in production.

### File System Access

The application writes output files:

- Ensure output directories have appropriate permissions
- Be cautious of path traversal when specifying output paths
- Review output files for sensitive data before sharing

**Mitigation**: Use absolute paths and verify permissions on output directories.

### Dependency Security

We regularly audit our dependencies for known vulnerabilities:

```bash
# Check for security advisories
cargo audit
```

## Security Features

### Built-in Protections

1. **Safe Rust**: The application is written in Rust, providing memory safety guarantees
2. **Dependency Management**: We keep dependencies updated and monitor security advisories
3. **Input Validation**: URLs and parameters are validated before use
4. **Resource Limits**: Configurable limits prevent resource exhaustion
5. **TLS/SSL**: HTTPS connections use secure, up-to-date TLS implementations

### Sandboxing Recommendations

For maximum security, consider running gh0st in a sandboxed environment:

```bash
# Using Docker with limited privileges
docker run --rm \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --read-only \
  -v $(pwd)/output:/data:rw \
  gh0st https://example.com -o /data/results.csv --no-tui

# Using systemd service with restrictions
[Service]
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/gh0st
```

## Disclosure Policy

When we receive a security vulnerability report:

1. We will confirm the problem and determine affected versions
2. We will audit code to find similar problems
3. We will prepare fixes for all supported versions
4. We will release patches as quickly as possible
5. We will publish a security advisory

### Security Advisory Process

1. **Confidential Fix Development**: Fixes are developed privately
2. **Coordinated Disclosure**: We coordinate with the reporter on disclosure timing
3. **Public Advisory**: Once fixed, we publish a security advisory with:
   - Description of the vulnerability
   - Affected versions
   - Fixed versions
   - Credit to the reporter (if desired)
   - Mitigation steps for users who cannot upgrade immediately

## Security Updates

Subscribe to security updates:

- Watch the [GitHub repository](https://github.com/yourusername/gh0st) for release notifications
- Check the [CHANGELOG](CHANGELOG.md) for security-related updates
- Monitor GitHub Security Advisories for the project

## Compliance

### GDPR and Data Privacy

When crawling websites, gh0st may collect personal data. Users are responsible for:

- Ensuring they have legal basis to crawl target websites
- Complying with data protection regulations (GDPR, CCPA, etc.)
- Implementing appropriate data handling and retention policies
- Obtaining necessary consents when required

### Responsible Disclosure

We follow responsible disclosure principles and request that security researchers do the same:

- Allow reasonable time for fixes before public disclosure (typically 90 days)
- Avoid privacy violations and destruction of data
- Don't exploit vulnerabilities beyond proof-of-concept

## Security Checklist

Before deploying gh0st in production:

- [ ] Running the latest stable version
- [ ] Output files have restricted permissions
- [ ] WebDriver endpoints are properly secured
- [ ] Input URLs are validated and sanitized
- [ ] Resource limits are configured appropriately
- [ ] Network access is restricted as needed
- [ ] Logs are monitored for suspicious activity
- [ ] Security advisories are being monitored

## Contact

For security issues: security@example.com (replace with your actual contact)

For general issues: Use [GitHub Issues](https://github.com/yourusername/gh0st/issues)

## References

- [OWASP Web Application Security](https://owasp.org/)
- [Rust Security Database](https://rustsec.org/)
- [CVE Database](https://cve.mitre.org/)

---

Last updated: 2026-02-19
