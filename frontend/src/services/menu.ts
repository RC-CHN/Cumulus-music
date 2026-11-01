export interface MenuLeaf {
  key: string;
  label: string;
  icon?: string;
  disabled?: boolean;
  checked?: boolean;
  checkKind?: 'radio' | 'checkbox';
  danger?: boolean;
  separator?: boolean;
  hint?: string;
}
export interface MenuItem extends MenuLeaf {
  children?: MenuLeaf[];
}
