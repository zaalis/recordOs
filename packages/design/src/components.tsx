import {
  type ButtonHTMLAttributes,
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { createPortal } from 'react-dom';
import { Icon, type IconName } from './icons.tsx';

type Variant = 'default' | 'primary' | 'danger' | 'ghost';
type Size = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  block?: boolean;
  icon?: IconName;
  loading?: boolean;
}

export function Button({ variant = 'default', size = 'md', block, icon, loading, children, className, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      className={['vp-button', className].filter(Boolean).join(' ')}
      data-variant={variant}
      data-size={size}
      data-block={block ? 'true' : undefined}
      {...rest}
    >
      {loading ? <span className="vp-spinner" style={{ width: 16, height: 16 }} /> : icon ? <Icon name={icon} size={size === 'sm' ? 16 : 20} /> : null}
      {children}
    </button>
  );
}

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  label: string;
  variant?: Variant;
  size?: 'sm' | 'md';
  active?: boolean;
}

export function IconButton({ icon, label, variant = 'ghost', size = 'md', active, className, ...rest }: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={['vp-button vp-icon-button', className].filter(Boolean).join(' ')}
      data-variant={active ? 'default' : variant}
      data-size={size}
      aria-pressed={active}
      {...rest}
    >
      <Icon name={icon} size={size === 'sm' ? 18 : 22} />
    </button>
  );
}

export interface SegmentedOption<T extends string | number> {
  value: T;
  label: ReactNode;
  disabled?: boolean;
}

export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  block,
  ariaLabel,
}: {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  block?: boolean;
  ariaLabel?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState<{ x: number; w: number } | null>(null);

  useLayoutEffect(() => {
    const root = ref.current;
    if (!root) return;
    const update = () => {
      const index = options.findIndex((o) => o.value === value);
      const el = root.querySelectorAll<HTMLButtonElement>('.vp-segment')[index];
      if (el) setThumb({ x: el.offsetLeft, w: el.offsetWidth });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(root);
    return () => observer.disconnect();
  }, [value, options]);

  return (
    <div ref={ref} className="vp-segmented" role="radiogroup" aria-label={ariaLabel} data-block={block ? 'true' : undefined}>
      {thumb && <span className="vp-segmented-thumb" style={{ width: thumb.w, transform: `translateX(${thumb.x - 2}px)`, left: 2 }} />}
      {options.map((option) => (
        <button
          key={String(option.value)}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          disabled={option.disabled}
          className="vp-segment"
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (checked: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className="vp-switch"
      onClick={() => onChange(!checked)}
    />
  );
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  label,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  label: string;
}) {
  const fill = ((value - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      className="vp-slider"
      aria-label={label}
      min={min}
      max={max}
      step={step}
      value={value}
      style={{ ['--fill' as string]: `${fill}%` }}
      onChange={(e) => onChange(Number(e.currentTarget.value))}
    />
  );
}

export function Group({ title, footer, children }: { title?: ReactNode; footer?: ReactNode; children: ReactNode }) {
  return (
    <section className="vp-group">
      {title && <div className="vp-overline vp-group-title">{title}</div>}
      <div className="vp-list">{children}</div>
      {footer && <div className="vp-footnote">{footer}</div>}
    </section>
  );
}

export function Row({
  icon,
  title,
  subtitle,
  value,
  accessory,
  onClick,
  chevron,
  tone,
}: {
  icon?: IconName | ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  value?: ReactNode;
  accessory?: ReactNode;
  onClick?: () => void;
  chevron?: boolean;
  tone?: 'red';
}) {
  const content = (
    <>
      {icon && (
        <span style={{ display: 'grid', placeItems: 'center', color: tone === 'red' ? 'var(--vp-red)' : 'var(--vp-text-2)' }}>
          {typeof icon === 'string' ? <Icon name={icon as IconName} size={20} /> : icon}
        </span>
      )}
      <span className="vp-row-body">
        <span className="vp-row-title" style={tone === 'red' ? { color: 'var(--vp-red)' } : undefined}>
          {title}
        </span>
        {subtitle && <span className="vp-row-subtitle">{subtitle}</span>}
      </span>
      {value !== undefined && <span className="vp-row-value">{value}</span>}
      {accessory}
      {chevron && <Icon name="chevron-right" size={16} style={{ color: 'var(--vp-text-3)' }} />}
    </>
  );
  return onClick ? (
    <button type="button" className="vp-row" onClick={onClick}>
      {content}
    </button>
  ) : (
    <div className="vp-row">{content}</div>
  );
}

export function Badge({ tone, children, dot }: { tone?: 'green' | 'red' | 'orange' | 'blue'; children: ReactNode; dot?: boolean }) {
  return (
    <span className="vp-badge" data-tone={tone}>
      {dot && <span className="vp-dot" data-tone={tone} style={{ width: 6, height: 6, boxShadow: 'none' }} />}
      {children}
    </span>
  );
}

export function Spinner({ size = 20 }: { size?: number }) {
  return <span className="vp-spinner" style={{ width: size, height: size }} role="progressbar" aria-label="Loading" />;
}

export function KeyCap({ children }: { children: ReactNode }) {
  return <kbd className="vp-keycap">{children}</kbd>;
}

export function Sheet({
  open,
  title,
  onClose,
  children,
  width,
  actions,
}: {
  open: boolean;
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  width?: number;
  actions?: ReactNode;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);
  if (!open) return null;
  return createPortal(
    <>
      <div className="vp-scrim" onClick={onClose} />
      <div className="vp-sheet" role="dialog" aria-modal="true" style={width ? { width: `min(${width}px, calc(100vw - 32px))` } : undefined}>
        <div className="vp-sheet-header">
          <h2 className="vp-title" style={{ flex: 1, fontSize: 20 }}>
            {title}
          </h2>
          {actions}
          <IconButton icon="close" label="Close" size="sm" variant="default" onClick={onClose} />
        </div>
        <div className="vp-sheet-body">{children}</div>
      </div>
    </>,
    document.body,
  );
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

export interface ToastOptions {
  tone?: 'green' | 'red' | 'orange' | 'blue';
  icon?: IconName;
  duration?: number;
}

interface ToastItem extends ToastOptions {
  id: number;
  message: ReactNode;
}

const ToastContext = createContext<(message: ReactNode, options?: ToastOptions) => void>(() => undefined);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const counter = useRef(0);
  const show = useCallback((message: ReactNode, options: ToastOptions = {}) => {
    const id = ++counter.current;
    setItems((list) => [...list.slice(-2), { id, message, ...options }]);
    window.setTimeout(() => setItems((list) => list.filter((t) => t.id !== id)), options.duration ?? 3200);
  }, []);
  return (
    <ToastContext.Provider value={show}>
      {children}
      {createPortal(
        <div className="vp-toast-region" role="status" aria-live="polite">
          {items.map((item) => (
            <div key={item.id} className="vp-toast">
              {item.icon && (
                <Icon
                  name={item.icon}
                  size={18}
                  style={{ color: item.tone ? `var(--vp-${item.tone})` : 'var(--vp-text-2)', flex: 'none' }}
                />
              )}
              <span>{item.message}</span>
            </div>
          ))}
        </div>,
        document.body,
      )}
    </ToastContext.Provider>
  );
}

export function useToast() {
  return useContext(ToastContext);
}
