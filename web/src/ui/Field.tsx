import { useId, type InputHTMLAttributes, type ReactNode, type Ref, type SelectHTMLAttributes } from 'react'

interface FieldShellProps {
  label: ReactNode
  hint?: ReactNode
  error?: string | null
  id: string
  children: ReactNode
  className?: string
}

function FieldShell({ label, hint, error, id, children, className }: FieldShellProps) {
  return (
    <div className={['field', error && 'field--invalid', className].filter(Boolean).join(' ')}>
      <label className="field__label" htmlFor={id}>
        {label}
      </label>
      {children}
      {error ? (
        <p className="field__error" id={`${id}-error`} role="alert">
          {error}
        </p>
      ) : (
        hint && (
          <p className="field__hint" id={`${id}-hint`}>
            {hint}
          </p>
        )
      )}
    </div>
  )
}

export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'id'> {
  label: ReactNode
  hint?: ReactNode
  error?: string | null
  fieldClassName?: string
  ref?: Ref<HTMLInputElement>
}

export function TextField({ label, hint, error, fieldClassName, className, ...props }: TextFieldProps) {
  const id = useId()
  return (
    <FieldShell label={label} hint={hint} error={error} id={id} className={fieldClassName}>
      <input
        id={id}
        className={['input', className].filter(Boolean).join(' ')}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? `${id}-error` : hint ? `${id}-hint` : undefined}
        {...props}
      />
    </FieldShell>
  )
}

export interface SelectFieldProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'id'> {
  label: ReactNode
  hint?: ReactNode
  error?: string | null
  fieldClassName?: string
}

export function SelectField({ label, hint, error, fieldClassName, className, children, ...props }: SelectFieldProps) {
  const id = useId()
  return (
    <FieldShell label={label} hint={hint} error={error} id={id} className={fieldClassName}>
      <select id={id} className={['input select', className].filter(Boolean).join(' ')} {...props}>
        {children}
      </select>
    </FieldShell>
  )
}

export function Checkbox({ label, ...props }: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & { label: ReactNode }) {
  return (
    <label className="checkbox">
      <input type="checkbox" {...props} />
      <span>{label}</span>
    </label>
  )
}

export function FormError({ error }: { error: string | null | undefined }) {
  if (!error) return null
  return (
    <p className="form-error" role="alert">
      {error}
    </p>
  )
}
