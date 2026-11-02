use crate::ui;

/// Optional Cloudflare DNS + HTTPS checklist (not a run target).
pub fn print_cloudflare_checklist(domain: &str, target_ip_or_host: &str, origin_port: u16) {
    ui::heading("Optional domain / Cloudflare");
    ui::info(format!("Domain: {domain}"));
    ui::info(format!("Point DNS at: {target_ip_or_host}"));
    println!(
        "\n1. In Cloudflare DNS, add an A (or CNAME) record for `{domain}` → `{target_ip_or_host}`.\n\
         2. Start with the proxy cloud grey (DNS only) until HTTP works on the origin.\n\
         3. SSL/TLS: prefer Full (strict) once you terminate HTTPS on the origin (Caddy/nginx).\n\
            Avoid Flexible (encrypts browser→Cloudflare only; origin stays plain HTTP).\n\
         4. Recommended hardening for remote_run: bind yourai to 127.0.0.1 and put Caddy in front:\n\
              yourai on 127.0.0.1:{origin_port}  ←  Caddy serves https://{domain}\n\
         5. Open only 80/443 on the firewall once HTTPS is working; keep {origin_port} private.\n"
    );
}

pub fn readme_dns_section(domain: &str, target: &str, port: u16) -> String {
    if domain.is_empty() {
        return String::new();
    }
    format!(
        "## Domain / Cloudflare\n\n\
         - Domain: `{domain}`\n\
         - DNS target: `{target}`\n\
         - Origin port: `{port}`\n\
         - Grey-cloud A/CNAME first; then Full (strict) SSL with Caddy/nginx on the origin.\n\
         - Do not treat Cloudflare as where the model runs — it is only DNS/HTTPS in front.\n\n"
    )
}
