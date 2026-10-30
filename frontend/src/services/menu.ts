export interface MenuItem {
  key: string;
  label: string;
  icon?: string;
  disabled?: boolean;
  checked?: boolean;
  danger?: boolean;
  separator?: boolean;
  hint?: string;
}
