/** Styles native radio choices as a compact single-selection switcher without duplicating keyboard behavior. */
import { RadioGroup, type RadioGroupProps } from "./RadioGroup";

export function SegmentedControl<Value extends string>({ className, ...props }: RadioGroupProps<Value>) {
  return <RadioGroup {...props} className={`ui-segmented${className ? ` ${className}` : ""}`} />;
}
