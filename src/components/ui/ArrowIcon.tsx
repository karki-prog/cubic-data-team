/** Shared external/forward arrow — same on phone and desktop. */
export function ArrowIcon({
  className = "",
  color = "currentColor",
  size = 14,
}: {
  className?: string;
  color?: string;
  size?: number;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 14 14"
      fill="none"
      aria-hidden
      className={`inline-block shrink-0 ${className}`}
    >
      <path
        d="M4 10.5L10.5 4M10.5 4H5.5M10.5 4V9"
        stroke={color}
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
