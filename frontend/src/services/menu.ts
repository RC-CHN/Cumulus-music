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
  /** Read-only retry may replace the contents of the currently open panel. */
  keepOpen?: boolean;
}
export interface MenuItem extends MenuLeaf {
  children?: MenuLeaf[];
}
