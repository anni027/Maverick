/// Authored icon set — stroke-based, consistent 1.4–1.6 weight, sized per use.
/// No emoji, no generic icon fonts (PRODUCT.md rule).

interface IconProps {
  size?: number;
  className?: string;
}

const base = (size: number) => ({
  width: size,
  height: size,
  viewBox: '0 0 16 16',
  fill: 'none' as const,
  stroke: 'currentColor',
  strokeWidth: 1.4,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
  className: undefined as string | undefined,
});

export const PlusIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M8 3v10M3 8h10" /></svg>
);

export const PanelIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><rect x="2.5" y="3" width="11" height="10" rx="1.5" /><path d="M6 3v10" /></svg>
);

export const FolderIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.4 1.8h5A1.5 1.5 0 0 1 14 6.3v5.2a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z" /></svg>
);

export const SettingsIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <circle cx="8" cy="8" r="2.2" />
    <path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6" />
    <path d="M3.7 3.7l1.1 1.1M11.2 11.2l1.1 1.1M12.3 3.7l-1.1 1.1M4.8 11.2l-1.1 1.1" opacity="0.6" />
  </svg>
);

export const MoreIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><circle cx="3.5" cy="8" r="0.9" fill="currentColor" stroke="none" /><circle cx="8" cy="8" r="0.9" fill="currentColor" stroke="none" /><circle cx="12.5" cy="8" r="0.9" fill="currentColor" stroke="none" /></svg>
);

export const ChevronIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
);

export const AttachIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M11.5 7.5 8 11a2.5 2.5 0 0 1-3.5-3.5l4.2-4.2a1.8 1.8 0 0 1 2.6 2.6L7 10.2" /></svg>
);

export const ToolsIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M9.5 6.5 3 13M12.6 3.4a3.4 3.4 0 0 1-4.6 4.2L6.5 9.1" /><path d="M11 2l3 3-1.4 1.4-3-3z" /></svg>
);

export const GlobeIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><circle cx="8" cy="8" r="5.5" /><path d="M2.5 8h11M8 2.5c1.6 1.5 2.4 3.4 2.4 5.5S9.6 12 8 13.5C6.4 12 5.6 10.1 5.6 8S6.4 4 8 2.5z" /></svg>
);

export const BrainIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M6.5 3.2a2 2 0 0 0-2 1.8 2 2 0 0 0-1 3.4A2 2 0 0 0 4.5 12a2 2 0 0 0 2 .9zM9.5 3.2a2 2 0 0 1 2 1.8 2 2 0 0 1 1 3.4A2 2 0 0 1 11.5 12a2 2 0 0 1-2 .9z" /><path d="M8 3v10" opacity="0.5" /></svg>
);

export const SendIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} strokeWidth={1.8} className={className}><path d="M8 13V3M3.5 7.5 8 3l4.5 4.5" /></svg>
);

export const StopIcon = ({ size = 12, className }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 12 12" fill="currentColor" aria-hidden className={className}><rect x="2.5" y="2.5" width="7" height="7" rx="1.2" /></svg>
);

export const CopyIcon = ({ size = 12, className }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth={1.3} aria-hidden className={className}><rect x="4" y="4" width="6" height="6" rx="1" /><path d="M3 8H2.5A1 1 0 0 1 1.5 7V2.5A1 1 0 0 1 2.5 1.5H7A1 1 0 0 1 8 2.5V3" /></svg>
);

export const CheckIcon = ({ size = 12, className }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden className={className}><path d="M2.5 6.5 4.5 8.5 9.5 3.5" /></svg>
);

export const FileIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M9.5 2H5A1.5 1.5 0 0 0 3.5 3.5v9A1.5 1.5 0 0 0 5 14h6a1.5 1.5 0 0 0 1.5-1.5V5.5z" /><path d="M9.5 2v3.5H13" /></svg>
);

export const PlugIcon = ({ size = 16, className }: IconProps) => (
  <svg {...base(size)} className={className}><path d="M6 2v3M10 2v3M4.5 5h7v1.5a3.5 3.5 0 0 1-7 0zM8 9.5V14" /></svg>
);

export const XIcon = ({ size = 10, className }: IconProps) => (
  <svg {...base(size)} strokeWidth={1.8} className={className}><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" /></svg>
);
