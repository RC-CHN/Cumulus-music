/* Original CRT static-SSE cosf/sinf paths at 14E580/14EB80. This bounded
 * translation is only used for positive GEQ phases <=0.9*pi (the proven
 * coefficient clamp). It has no large-angle reduction, AVX dispatch, NaN
 * diagnostics, OS imports or general math-library ABI. Keep separate double
 * operations and -ffp-contract=off; constants are transcribed as exact f64.
 */
static double filter_peak_sine_polynomial(double x) {
    double square=x*x,fourth=square*square;
    double a=2.7557319223985893e-6*square;
    double b=0.008333333333333333*square;
    a=a+(-0.0001984126984126984);a=a*fourth;
    b=b+(-0.16666666666666666);a=a+b;
    double cubic=x*square;a=a*cubic;return x+a;
}
static double filter_peak_cosine_polynomial(double x) {
    double square=x*x,fourth=square*square;
    double a=-2.755731922398589e-7*square;
    double b=-0.0013888888888888887*square;
    double c=-0.5*square;
    a=a+2.4801587301587298e-5;a=a*fourth;
    b=b+0.041666666666666664;a=a+b;a=a*fourth;
    c=c+1.0;return c+a;
}
static unsigned filter_peak_exponent(double x) {
    uint64_t bits;memcpy(&bits,&x,sizeof(bits));return (unsigned)((bits>>52)&2047u);
}
static double filter_peak_reduce(double x,unsigned *quadrant,unsigned *small) {
    double t=x*0.6366197723675814;t=t+0.5;unsigned n=(unsigned)t;
    double high=x-(double)n*1.5707963267341256;
    double low=(double)n*6.077100506506192e-11;
    double r=high-low;unsigned exponent=filter_peak_exponent(r);
    if(filter_peak_exponent(x)-exponent>15) {
        double before=high,part=(double)n*6.077100506303966e-11;
        double tail=(double)n*2.0222662487959506e-21;
        high=high-part;before=before-high;before=before-part;
        tail=tail-before;r=high-tail;
    }
    *quadrant=n;*small=exponent;return r;
}
static void filter_peak_trig(float phase,float *sine,float *cosine) {
    if(!isfinite(phase) || phase<0.0f || phase>3.0f){*sine=NAN;*cosine=NAN;return;}
    double x=(double)phase,s,c;unsigned quadrant=0,exponent=2047;
    if(float_bits(phase)<=0x3f490fdbu) {
        c=filter_peak_cosine_polynomial(x);
        if(float_bits(phase)<0x39000000u)s=x;
        else if(float_bits(phase)<0x3c000000u) {
            double cubic=x*x;cubic=cubic*x;cubic=cubic*0.16666666666666666;s=x-cubic;
        } else s=filter_peak_sine_polynomial(x);
    } else {
        x=filter_peak_reduce(x,&quadrant,&exponent);
        if(exponent<0x3f2u) {
            double sin_small=x,cos_small=1.0;
            if(exponent>0x3deu) {
                double square=x*x,term=0.16666666666666666*x;
                term=term*square;sin_small=x-term;cos_small=1.0-square*0.5;
            }
            s=quadrant&1u?cos_small:sin_small;
            c=quadrant&1u?sin_small:cos_small;
        } else {
            s=quadrant&1u?filter_peak_cosine_polynomial(x):filter_peak_sine_polynomial(x);
            c=quadrant&1u?filter_peak_sine_polynomial(x):filter_peak_cosine_polynomial(x);
        }
        if(quadrant&2u)s=0.0-s;
        if((quadrant+1u)&2u)c=0.0-c;
    }
    *sine=(float)s;*cosine=(float)c;
}
