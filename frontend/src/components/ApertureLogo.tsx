/// Aperture brand mark — seven gradient blades on a black tile.
/// `animated` spins the blade group (in-flight indicator); static otherwise.
/// Reduced-motion users get a static mark (see index.css media query).

interface ApertureLogoProps {
  size?: number;
  animated?: boolean;
  className?: string;
}

const BLADE_POINTS = '0,-46 44,-162 76,-88 18,-34';
const BLADE_ROTATIONS = [0, 51.4286, 102.8571, 154.2857, 205.7143, 257.1426, 308.5714];

export default function ApertureLogo({ size = 24, animated = false, className }: ApertureLogoProps) {
  const gradId = animated ? 'apertureGradAnim' : 'apertureGrad';
  return (
    <svg width={size} height={size} viewBox="0 0 500 500" className={className} aria-hidden>
      <defs>
        <linearGradient id={gradId} x1="0%" x2="100%" y1="100%" y2="0%">
          <stop offset="0%" stopColor="var(--faint)" />
          <stop offset="50%" stopColor="var(--muted)" />
          <stop offset="100%" stopColor="var(--text)" />
        </linearGradient>
      </defs>
      <g transform="translate(250, 250)" className={animated ? 'aperture-spin' : undefined}>
        {BLADE_ROTATIONS.map(rot => (
          <g key={rot} transform={`rotate(${rot})`}>
            <polygon fill={`url(#${gradId})`} points={BLADE_POINTS} />
          </g>
        ))}
      </g>
    </svg>
  );
}

/// Aperture on a quiet rounded tile — avatar/sidebar brand usage.
export function ApertureTile({ size = 28, animated = false }: { size?: number; animated?: boolean }) {
  return (
    <div
      style={{
        width: size,
        height: size,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'var(--panel)',
        border: '1px solid var(--line)',
        borderRadius: '50%',
        padding: Math.round(size * 0.16),
      }}
    >
      <ApertureLogo size={size - Math.round(size * 0.32)} animated={animated} />
    </div>
  );
}
