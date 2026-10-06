/** Shimmering placeholder block. Styles live in globals.css (.cubic-skel). */
export function Skel({
  w = "100%",
  h = 12,
  r,
  className = "",
  style,
}: {
  w?: number | string;
  h?: number | string;
  r?: number | string;
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <span
      aria-hidden
      className={`cubic-skel${className ? ` ${className}` : ""}`}
      style={{ width: w, height: h, borderRadius: r, ...style }}
    />
  );
}

/** Generic page skeleton: header strip + content cards. Used while auth is checked. */
export function PageSkeleton() {
  return (
    <main
      aria-busy="true"
      aria-label="Loading"
      style={{ minHeight: "100dvh", background: "#f5f6f8", display: "flex", flexDirection: "column" }}
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr auto 1fr",
          alignItems: "center",
          gap: 16,
          padding: "12px 20px",
          background: "#fff",
          borderBottom: "2px solid #c5221f",
        }}
      >
        <Skel w={90} h={26} />
        <Skel w={260} h={24} />
        <Skel w={140} h={34} style={{ justifySelf: "end" }} />
      </div>
      <div style={{ padding: "16px 20px", display: "grid", gap: 16 }}>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", gap: 12 }}>
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="cubic-skel-card" style={{ padding: 16, display: "grid", gap: 10 }}>
              <Skel w="55%" h={10} />
              <Skel w="40%" h={24} />
            </div>
          ))}
        </div>
        <div className="cubic-skel-card">
          <div style={{ padding: "16px 22px", background: "#e9ecef" }}>
            <Skel w={180} h={14} style={{ margin: "0 auto", background: "#dde1e6" }} />
          </div>
          {Array.from({ length: 8 }).map((_, i) => (
            <div
              key={i}
              style={{ display: "grid", gridTemplateColumns: "40px 1.4fr 1fr 1fr", gap: 18, padding: "14px 20px", borderTop: "1px solid #eef0f3" }}
            >
              <Skel w={18} h={10} />
              <Skel w="70%" h={12} />
              <Skel w="50%" h={12} />
              <Skel w="40%" h={12} />
            </div>
          ))}
        </div>
      </div>
    </main>
  );
}
