// Identifiers live only for this modal. Passwords/verification answers never enter it.
export interface LoginIdentity {
  phone: string;
  country: string;
  countryAlpha: string;
  consent: boolean;
  email: string;
  remember: boolean;
}
