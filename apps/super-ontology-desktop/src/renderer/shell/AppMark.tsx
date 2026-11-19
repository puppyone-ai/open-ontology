/** The product mark: three nodes joined by lines, axis-symmetric, drawn on the 20px grid in `currentColor`. */
export function AppMark({ size = 20 }: { size?: 16 | 20 | 28 }) {
  return (
    <svg className="so-app-mark" width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true" focusable="false">
      <path d="M10 5.25V10M10 10L5.5 14.25M10 10l4.5 4.25" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="10" cy="4" r="2" fill="currentColor" />
      <circle cx="4.5" cy="15.25" r="2" fill="currentColor" />
      <circle cx="15.5" cy="15.25" r="2" fill="currentColor" />
    </svg>
  );
}
