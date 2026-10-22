import { Button } from '@vision-pro/design';
import { useEffect, useRef, useState } from 'react';

/** Four single-digit fields for the code shown on the PC. Pasting or autofilling all four works. */
export function CodeEntry({ onSubmit, autoFocus = false, label = 'Connecter', disabled = false }: { onSubmit: (code: string) => void; autoFocus?: boolean; disabled?: boolean; label?: string }) {
  const [digits, setDigits] = useState<string[]>(Array(4).fill(''));
  const refs = useRef<(HTMLInputElement | null)[]>([]);
  const code = digits.join('');

  useEffect(() => {
    if (!autoFocus) return;
    const timer = window.setTimeout(() => refs.current[0]?.focus(), 250);
    return () => window.clearTimeout(timer);
  }, [autoFocus]);

  const submit = (value = code) => {
    if (disabled || value.length !== 4) return;
    onSubmit(value);
    setDigits(Array(4).fill(''));
  };

  const setDigit = (index: number, value: string) => {
    const clean = value.replace(/\D/g, '');
    if (clean.length > 1) {
      const next = clean.slice(0, 4).split('');
      const filled = [...next, ...Array(4 - next.length).fill('')];
      setDigits(filled);
      refs.current[Math.min(3, next.length)]?.focus();
      if (next.length === 4) submit(next.join(''));
      return;
    }
    const next = [...digits];
    next[index] = clean;
    setDigits(next);
    if (clean && index < 3) refs.current[index + 1]?.focus();
    if (clean && index === 3 && next.every(Boolean)) submit(next.join(''));
  };

  return (
    <>
      <div className="code-inputs">
        {digits.map((digit, index) => (
          <input
            key={index}
            ref={(el) => {
              refs.current[index] = el;
            }}
            disabled={disabled}
            inputMode="numeric"
            pattern="[0-9]*"
            autoComplete={index === 0 ? 'one-time-code' : 'off'}
            aria-label={`Chiffre ${index + 1} du code`}
            value={digit}
            maxLength={4}
            onChange={(e) => setDigit(index, e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Backspace' && !digit && index > 0) refs.current[index - 1]?.focus();
              if (e.key === 'Enter') submit();
            }}
          />
        ))}
      </div>
      <Button variant="primary" size="lg" block disabled={disabled || code.length !== 4} onClick={() => submit()}>
        {label}
      </Button>
    </>
  );
}
