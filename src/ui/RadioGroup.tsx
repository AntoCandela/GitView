/** Renders controlled native radio choices with one tab stop and explicit keyboard selection. */

import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { Tooltip } from "./Tooltip";
import { useTranslation } from "../i18n";

export interface RadioGroupOption<Value extends string> {
  value: Value;
  label: string;
  disabled?: boolean;
  content?: ReactNode;
}

export interface RadioGroupProps<Value extends string> {
  label: string;
  value: Value;
  options: readonly RadioGroupOption<Value>[];
  onChange: (value: Value) => void;
  /** Optional renewed intent when the already-selected option is activated. */
  onReselect?: (value: Value) => void;
  disabled?: boolean;
  className?: string;
}

/** Values identify choices within this group; native grouping stays unique across mounted groups. */
export function RadioGroup<Value extends string>({
  label,
  value,
  options,
  onChange,
  onReselect,
  disabled = false,
  className,
}: RadioGroupProps<Value>) {
  const { t } = useTranslation();
  const name = useId();
  const inputs = useRef(new Map<Value, HTMLInputElement>());
  const enabledOptions = disabled ? [] : options.filter((option) => !option.disabled);
  const tabValue = enabledOptions.find((option) => option.value === value)?.value
    ?? enabledOptions[0]?.value;

  function selectWithKeyboard(event: KeyboardEvent<HTMLInputElement>, optionValue: Value) {
    const currentIndex = enabledOptions.findIndex((option) => option.value === optionValue);
    if (currentIndex < 0) return;

    let nextIndex: number;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        nextIndex = (currentIndex + 1) % enabledOptions.length;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        nextIndex = (currentIndex + enabledOptions.length - 1) % enabledOptions.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = enabledOptions.length - 1;
        break;
      case " ":
        nextIndex = currentIndex;
        break;
      default:
        return;
    }
    event.preventDefault();
    const nextValue = enabledOptions[nextIndex].value;
    inputs.current.get(nextValue)?.focus();
    if (nextValue !== value) onChange(nextValue);
    else onReselect?.(nextValue);
  }

  return (
    <div
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled || undefined}
      className={`ui-radio-group${className ? ` ${className}` : ""}`}
    >
      {options.map((option) => (
        <Tooltip key={option.value} content={t("ui.setOption", { group: label, option: option.label })} trigger={
        <label className="ui-radio-option">
          <input
            ref={(input) => {
              if (input) inputs.current.set(option.value, input);
              else inputs.current.delete(option.value);
            }}
            type="radio"
            name={name}
            value={option.value}
            aria-label={option.label}
            checked={option.value === value}
            disabled={disabled || option.disabled}
            tabIndex={option.value === tabValue ? 0 : -1}
            onChange={() => onChange(option.value)}
            onClick={() => { if (option.value === value) onReselect?.(option.value); }}
            onKeyDown={(event) => selectWithKeyboard(event, option.value)}
          />
          <span className="ui-radio-content">{option.content ?? option.label}</span>
        </label>} />
      ))}
    </div>
  );
}
