/** Inline SVG art for each dashboard tile — Cubic red / slate palette. */
export function ToolGraphic({ id, muted = false }: { id: string; muted?: boolean }) {
  const red = muted ? "#c4c9ce" : "#c5221f";
  const soft = muted ? "#e8eaed" : "#fdecea";
  const ink = muted ? "#9aa0a6" : "#1a1a1a";
  const green = muted ? "#b8c4bc" : "#157f3d";

  switch (id) {
    case "daily-apply-list":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="8" y="10" width="104" height="68" rx="6" fill={soft} />
          <rect x="18" y="20" width="52" height="6" rx="2" fill={red} />
          <rect x="18" y="34" width="84" height="4" rx="2" fill={ink} opacity="0.25" />
          <rect x="18" y="44" width="72" height="4" rx="2" fill={ink} opacity="0.2" />
          <rect x="18" y="54" width="78" height="4" rx="2" fill={ink} opacity="0.15" />
          <circle cx="92" cy="58" r="14" fill={green} opacity="0.9" />
          <path d="M86 58l4 4 8-9" stroke="#fff" strokeWidth="2.5" fill="none" strokeLinecap="round" />
        </svg>
      );
    case "cubic-interviews":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="10" y="12" width="100" height="64" rx="8" fill={soft} />
          {[0, 1, 2].map((r) => (
            <g key={r}>
              <rect x="20" y={24 + r * 16} width="38" height="5" rx="2" fill={ink} opacity={0.24 - r * 0.04} />
              <rect x="66" y={22 + r * 16} width={30 - r * 8} height="9" rx="4.5" fill={r === 0 ? green : red} opacity={r === 0 ? 0.9 : 0.55} />
            </g>
          ))}
          <circle cx="96" cy="62" r="11" fill={red} />
          <circle cx="96" cy="59" r="3.5" fill="#fff" />
          <path d="M89.5 68.5c1.4-3 3.8-4.5 6.5-4.5s5.1 1.5 6.5 4.5" stroke="#fff" strokeWidth="2" fill="none" strokeLinecap="round" />
        </svg>
      );
    case "do-not-apply":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="14" y="12" width="92" height="64" rx="8" fill={soft} />
          <rect x="24" y="24" width="48" height="5" rx="2" fill={red} opacity="0.85" />
          <rect x="24" y="36" width="64" height="4" rx="2" fill={ink} opacity="0.2" />
          <rect x="24" y="46" width="56" height="4" rx="2" fill={ink} opacity="0.16" />
          <rect x="24" y="56" width="60" height="4" rx="2" fill={ink} opacity="0.12" />
          <circle cx="88" cy="52" r="16" fill={red} opacity="0.92" />
          <circle cx="88" cy="52" r="10" stroke="#fff" strokeWidth="2.4" fill="none" />
          <path d="M81 45l14 14" stroke="#fff" strokeWidth="2.4" strokeLinecap="round" />
        </svg>
      );
    case "interview-booking":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="22" y="14" width="76" height="60" rx="8" fill={soft} />
          <rect x="22" y="14" width="76" height="16" rx="8" fill={red} />
          <rect x="22" y="24" width="76" height="6" fill={red} />
          {[0, 1, 2, 3].map((c) =>
            [0, 1, 2].map((r) => (
              <rect
                key={`${c}-${r}`}
                x={32 + c * 16}
                y={36 + r * 12}
                width="10"
                height="8"
                rx="2"
                fill={r === 1 && c === 2 ? green : ink}
                opacity={r === 1 && c === 2 ? 1 : 0.18}
              />
            ))
          )}
        </svg>
      );
    case "phone-call-booking":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <circle cx="60" cy="44" r="30" fill={soft} />
          {/* Material-style call handset, scaled & centered */}
          <g transform="translate(36 20) scale(2)">
            <path
              fill={red}
              d="M6.62 10.79c1.44 2.83 3.76 5.14 6.59 6.59l2.2-2.2c.27-.27.67-.36 1.02-.24 1.12.37 2.33.57 3.57.57.55 0 1 .45 1 1V20c0 .55-.45 1-1 1-9.39 0-17-7.61-17-17 0-.55.45-1 1-1h3.5c.55 0 1 .45 1 1 0 1.25.2 2.45.57 3.57.11.35.03.74-.25 1.02l-2.2 2.2z"
            />
          </g>
          <path
            d="M78 28c6 3 11 8 14 14M74 34c4 2 7 5 9 9"
            stroke={green}
            strokeWidth="2.5"
            fill="none"
            strokeLinecap="round"
            opacity={muted ? 0.5 : 1}
          />
        </svg>
      );
    case "application-tracker":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="20" y="48" width="14" height="22" rx="2" fill={ink} opacity="0.15" />
          <rect x="40" y="36" width="14" height="34" rx="2" fill={ink} opacity="0.22" />
          <rect x="60" y="28" width="14" height="42" rx="2" fill={red} opacity="0.55" />
          <rect x="80" y="20" width="14" height="50" rx="2" fill={green} opacity="0.75" />
          <path d="M24 52h80" stroke={ink} strokeWidth="1.5" opacity="0.12" />
        </svg>
      );
    case "tool-4":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <circle cx="60" cy="44" r="26" fill={soft} />
          <path
            d="M60 24v8M60 56v8M36 44h8M76 44h8M42 28l6 6M72 54l6 6M42 60l6-6M72 34l6-6"
            stroke={red}
            strokeWidth="3"
            strokeLinecap="round"
            opacity="0.7"
          />
          <circle cx="60" cy="44" r="8" fill={red} opacity="0.85" />
        </svg>
      );
    case "assessment-booking":
    case "assessment-apply":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          {/* Clipboard / assessment form */}
          <rect x="28" y="14" width="48" height="62" rx="6" fill={soft} />
          <rect x="40" y="10" width="24" height="10" rx="3" fill={red} />
          <rect x="36" y="30" width="20" height="4" rx="2" fill={ink} opacity="0.22" />
          <rect x="36" y="40" width="28" height="4" rx="2" fill={ink} opacity="0.16" />
          <rect x="36" y="50" width="24" height="4" rx="2" fill={ink} opacity="0.12" />
          <rect x="36" y="60" width="16" height="4" rx="2" fill={ink} opacity="0.1" />
          {/* Booking calendar badge */}
          <rect x="70" y="36" width="34" height="32" rx="5" fill={red} />
          <rect x="70" y="36" width="34" height="10" rx="5" fill={red} />
          <rect x="70" y="42" width="34" height="4" fill={red} />
          <circle cx="78" cy="56" r="2.2" fill="#fff" opacity="0.95" />
          <circle cx="87" cy="56" r="2.2" fill="#fff" opacity="0.95" />
          <circle cx="96" cy="56" r="2.2" fill={green} />
        </svg>
      );
    case "tool-5":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="20" y="48" width="14" height="22" rx="2" fill={ink} opacity="0.15" />
          <rect x="40" y="36" width="14" height="34" rx="2" fill={ink} opacity="0.22" />
          <rect x="60" y="28" width="14" height="42" rx="2" fill={red} opacity="0.55" />
          <rect x="80" y="20" width="14" height="50" rx="2" fill={green} opacity="0.75" />
          <path d="M24 52h80" stroke={ink} strokeWidth="1.5" opacity="0.12" />
        </svg>
      );
    case "resume-retargeting-prompt":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="24" y="10" width="56" height="68" rx="6" fill={soft} />
          <rect x="32" y="18" width="34" height="5" rx="2" fill={red} opacity="0.9" />
          <rect x="32" y="28" width="40" height="3" rx="1.5" fill={ink} opacity="0.2" />
          <rect x="32" y="35" width="36" height="3" rx="1.5" fill={ink} opacity="0.16" />
          <rect x="32" y="42" width="38" height="3" rx="1.5" fill={ink} opacity="0.14" />
          <rect x="32" y="49" width="30" height="3" rx="1.5" fill={ink} opacity="0.12" />
          <rect x="32" y="56" width="34" height="3" rx="1.5" fill={ink} opacity="0.1" />
          <circle cx="84" cy="54" r="16" fill={green} opacity="0.92" />
          <path
            d="M78 54l4 4 8-9"
            stroke="#fff"
            strokeWidth="2.5"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path
            d="M70 22l8-6 10 8"
            stroke={red}
            strokeWidth="2.2"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            opacity="0.75"
          />
        </svg>
      );
    case "onboarding-list":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="28" y="16" width="48" height="58" rx="5" fill={soft} />
          <path d="M76 16v14h14" fill={soft} opacity="0.7" />
          <path d="M76 16l14 14H76V16z" fill={red} opacity="0.35" />
          <rect x="36" y="30" width="28" height="3.5" rx="1.5" fill={ink} opacity="0.2" />
          <rect x="36" y="40" width="22" height="3.5" rx="1.5" fill={ink} opacity="0.15" />
          <rect x="36" y="50" width="26" height="3.5" rx="1.5" fill={ink} opacity="0.12" />
          <circle cx="86" cy="58" r="14" fill={green} opacity="0.9" />
          <path
            d="M86 50v11M81 57.5l5 5 5-5"
            stroke="#fff"
            strokeWidth="2.2"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      );
    case "practice-classes":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          {/* chalkboard / class board */}
          <rect x="22" y="14" width="76" height="48" rx="6" fill={soft} />
          <rect x="28" y="20" width="64" height="36" rx="3" fill={red} opacity="0.88" />
          <path
            d="M36 30h30M36 38h22M36 46h26"
            stroke="#fff"
            strokeWidth="2.4"
            strokeLinecap="round"
            opacity="0.9"
          />
          {/* chalk ledge */}
          <rect x="34" y="58" width="52" height="5" rx="1.5" fill={ink} opacity="0.18" />
          {/* students */}
          <circle cx="42" cy="72" r="5" fill={green} opacity="0.9" />
          <path d="M34 82c2-6 6-8 8-8s6 2 8 8" fill={green} opacity="0.55" />
          <circle cx="60" cy="72" r="5" fill={red} opacity="0.85" />
          <path d="M52 82c2-6 6-8 8-8s6 2 8 8" fill={red} opacity="0.45" />
          <circle cx="78" cy="72" r="5" fill={green} opacity="0.75" />
          <path d="M70 82c2-6 6-8 8-8s6 2 8 8" fill={green} opacity="0.45" />
        </svg>
      );
    case "support-feedback":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="18" y="12" width="58" height="64" rx="8" fill={soft} />
          <rect x="26" y="22" width="42" height="5" rx="2" fill={red} opacity="0.9" />
          <rect x="26" y="34" width="36" height="4" rx="2" fill={ink} opacity="0.2" />
          <rect x="26" y="44" width="40" height="4" rx="2" fill={ink} opacity="0.16" />
          <rect x="26" y="54" width="28" height="4" rx="2" fill={ink} opacity="0.12" />
          <path
            d="M78 28h22a6 6 0 016 6v22a6 6 0 01-6 6H92l-8 10v-10h-6a6 6 0 01-6-6V34a6 6 0 016-6z"
            fill={red}
            opacity="0.92"
          />
          <path
            d="M86 42h14M86 50h10"
            stroke="#fff"
            strokeWidth="2.4"
            strokeLinecap="round"
            opacity="0.95"
          />
          <circle cx="100" cy="64" r="3" fill={green} opacity={muted ? 0.5 : 1} />
        </svg>
      );
    case "tool-6":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="28" y="18" width="64" height="52" rx="6" fill={soft} />
          <circle cx="48" cy="40" r="10" fill={red} opacity="0.8" />
          <circle cx="72" cy="40" r="10" fill={green} opacity="0.75" />
          <path d="M40 58h40" stroke={ink} strokeWidth="3" strokeLinecap="round" opacity="0.2" />
        </svg>
      );
    case "tool-7":
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <path
            d="M24 58c12-22 24-34 36-34s24 12 36 34"
            fill="none"
            stroke={soft}
            strokeWidth="16"
            strokeLinecap="round"
          />
          <path
            d="M30 58c10-18 20-28 30-28s20 10 30 28"
            fill="none"
            stroke={red}
            strokeWidth="4"
            strokeLinecap="round"
            opacity="0.85"
          />
          <circle cx="60" cy="58" r="5" fill={green} />
        </svg>
      );
    default:
      return (
        <svg viewBox="0 0 120 88" className="h-full w-full" aria-hidden>
          <rect x="30" y="22" width="60" height="44" rx="8" fill={soft} />
          <path
            d="M52 44h16M60 36v16"
            stroke={red}
            strokeWidth="3"
            strokeLinecap="round"
            opacity="0.55"
          />
        </svg>
      );
  }
}
