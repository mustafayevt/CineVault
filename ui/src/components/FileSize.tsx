import { useFormat } from "../i18n/useFormat";

interface FileSizeProps {
  /** Size in bytes. */
  bytes: number;
  className?: string;
}

/**
 * A file size in the unit system the user chose in Settings (base 10: GB, base 2: GiB). Use it for
 * every size shown in the app so the preference applies everywhere.
 */
export function FileSize({ bytes, className }: FileSizeProps) {
  const format = useFormat();
  return (
    <span className={className} data-bytes={bytes}>
      {format.size(bytes)}
    </span>
  );
}
