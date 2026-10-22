import { Button } from '@vision-pro/design';
import { useEffect, useRef, useState } from 'react';

/** Six single-digit fields for the code shown on the PC. Pasting or autofilling all six works. */
export function CodeEntry({ onSubmit, autoFocus = false, label = 'Connecter' }: { onSubmit: (code: string) => void; autoFocus?: boolean; label?: string }) {
  const [digits, setDigits] = useState<string[]>(Array(6).fill(''));
  const refs = useRef<(HTMLInputElement | null)[]>([]);
  const code = digits.join('');

  useEffect(() => {
    if (!autoFocus) return;
    const timer = window.setTimeout(() => refs.current[0]?.focus(), 250);
    return () => window.clearTimeout(timer);
  }, [autoFocus]);

  const submit = (value = code) => {
    if (value.length !== 6) return;
    onSubmit(value);
    setDigits(Array(6).fill(''));
  };

  const setDigit = (index: number, value: string) => {
    const clean = value.replace(/\D/g, '');
    if (clean.length > 1) {
      const next = clean.slice(0, 6).split('');
      const filled = [...next, ...Array(6 - next.length).fill('')];
      setDigits(filled);
      refs.current[Math.min(5, next.length)]?.focus();
      if (next.length === 6) submit(next.join(''));
      return;
    }
    const next = [...digits];
    next[index] = clean;
    setDigits(next);
    if (clean && index < 5) refs.current[index + 1]?.focus();
    if (clean && index === 5 && next.every(Boolean)) submit(next.join(''));
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
            inputMode="numeric"
            pattern="[0-9]*"
            autoComplete={index === 0 ? 'one-time-code' : 'off'}
            aria-label={`Chiffre ${index + 1} du code`}
            value={digit}
            maxLength={6}
            onChange={(e) => setDigit(index, e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Backspace' && !digit && index > 0) refs.current[index - 1]?.focus();
              if (e.key === 'Enter') submit();
            }}
          />
        ))}
      </div>
      <Button variant="primary" size="lg" block disabled={code.length !== 6} onClick={() => submit()}>
        {label}
      </Button>
    </>
  );
}
