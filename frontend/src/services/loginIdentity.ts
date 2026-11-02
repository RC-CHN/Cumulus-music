// Identifiers live only for this modal. Passwords/verification answers never enter it.
export interface LoginIdentity {
  phone: string;
  country: string;
  email: string;
  remember: boolean;
}
