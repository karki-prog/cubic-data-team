type SiteFooterProps = {
  variant?: "drawer" | "bar";
};

export function SiteFooter({ variant = "drawer" }: SiteFooterProps) {
  const year = new Date().getFullYear();

  const mailSubject = "Cubic Data Dashboard issue";

  if (variant === "bar") {
    return (
      <footer className="cubic-site-footer-bar" aria-label="Site footer">
        <p className="cubic-site-footer-line cubic-site-footer-copy">
          © {year} Cubic Technologies. All rights reserved.
        </p>
        <p className="cubic-site-footer-line cubic-site-footer-contact">
          <span className="cubic-site-footer-label">Issues? Contact</span>{" "}
          <a href={`mailto:karki@cubicit.net?subject=${encodeURIComponent(mailSubject)}`}>
            Shaksham Karki
          </a>
          <span className="cubic-site-footer-sep" aria-hidden>
            {" "}
            or{" "}
          </span>
          <a
            className="cubic-site-footer-email"
            href={`mailto:karki@cubicit.net?subject=${encodeURIComponent(mailSubject)}`}
          >
            karki@cubicit.net
          </a>
        </p>
      </footer>
    );
  }

  return (
    <footer className="cubic-drawer-footer" aria-label="Site footer">
      <p className="cubic-drawer-footer-line">
        © {year} Cubic Technologies®
      </p>
      <p className="cubic-drawer-footer-line cubic-drawer-footer-contact">
        <span className="cubic-drawer-footer-label">Issues? Contact</span>{" "}
        <a href={`mailto:karki@cubicit.net?subject=${encodeURIComponent(mailSubject)}`}>
          Shaksham Karki
        </a>
        <span className="cubic-drawer-footer-sep" aria-hidden>
          {" "}
          ·{" "}
        </span>
        <a
          className="cubic-drawer-footer-email"
          href={`mailto:karki@cubicit.net?subject=${encodeURIComponent(mailSubject)}`}
        >
          karki@cubicit.net
        </a>
      </p>
    </footer>
  );
}
