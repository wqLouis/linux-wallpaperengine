
vec4 Desaturate(vec3 color, float Desaturation)
{
	vec3 grayXfer = vec3(0.3, 0.59, 0.11);
	vec3 gray = vec3(dot(grayXfer, color));
	return vec4(mix(color, gray, Desaturation), 1.0);
}

vec3 RGBToHSL(vec3 color)
{
	vec3 hsl;
	float fmin = min(min(color.r, color.g), color.b);
	float fmax = max(max(color.r, color.g), color.b);
	float delta = fmax - fmin;
	hsl.z = (fmax + fmin) / 2.0;

	if (delta == 0.0)
	{
		hsl.x = 0.0;
		hsl.y = 0.0;
	}
	else
	{
		if (hsl.z < 0.5)
			hsl.y = delta / (fmax + fmin);
		else
			hsl.y = delta / (2.0 - fmax - fmin);
		float deltaR = (((fmax - color.r) / 6.0) + (delta / 2.0)) / delta;
		float deltaG = (((fmax - color.g) / 6.0) + (delta / 2.0)) / delta;
		float deltaB = (((fmax - color.b) / 6.0) + (delta / 2.0)) / delta;
		if (color.r == fmax )
			hsl.x = deltaB - deltaG;
		else if (color.g == fmax)
			hsl.x = (1.0 / 3.0) + deltaR - deltaB;
		else if (color.b == fmax)
			hsl.x = (2.0 / 3.0) + deltaG - deltaR;

		if (hsl.x < 0.0)
			hsl.x += 1.0;
		else if (hsl.x > 1.0)
			hsl.x -= 1.0;
	}

	return hsl;
}

float HueToRGB(float f1, float f2, float hue)
{
	if (hue < 0.0)
		hue += 1.0;
	else if (hue > 1.0)
		hue -= 1.0;
	float res;
	if ((6.0 * hue) < 1.0)
		res = f1 + (f2 - f1) * 6.0 * hue;
	else if ((2.0 * hue) < 1.0)
		res = f2;
	else if ((3.0 * hue) < 2.0)
		res = f1 + (f2 - f1) * ((2.0 / 3.0) - hue) * 6.0;
	else
		res = f1;
	return res;
}

vec3 HSLToRGB(vec3 hsl)
{
	vec3 rgb;
	if (hsl.y == 0.0)
		rgb = vec3(hsl.z);
	else
	{
		float f2;
		if (hsl.z < 0.5)
			f2 = hsl.z * (1.0 + hsl.y);
		else
			f2 = (hsl.z + hsl.y) - (hsl.y * hsl.z);
		float f1 = 2.0 * hsl.z - f2;
		rgb.r = HueToRGB(f1, f2, hsl.x + (1.0/3.0));
		rgb.g = HueToRGB(f1, f2, hsl.x);
		rgb.b= HueToRGB(f1, f2, hsl.x - (1.0/3.0));
	}
	
	return rgb;
}

vec3 ContrastSaturationBrightness(vec3 color, float brt, float sat, float con)
{
	vec3 LumCoeff = vec3(0.2125, 0.7154, 0.0721);
	vec3 AvgLumin = vec3(0.5, 0.5, 0.5);
	vec3 brtColor = color * brt;
	vec3 intensity = vec3(dot(brtColor, LumCoeff));
	vec3 satColor = mix(intensity, brtColor, sat);
	vec3 conColor = mix(AvgLumin, satColor, con);
	return conColor;
}

// Blend helper functions
float BlendLinearDodgef(float base, float blend) { return base + blend; }
float BlendLinearBurnf(float base, float blend) { return max(base + blend - 1.0, 0.0); }
float BlendLightenf(float base, float blend) { return max(blend, base); }
float BlendDarkenf(float base, float blend) { return min(blend, base); }
float BlendLinearLightf(float base, float blend) { return blend < 0.5 ? BlendLinearBurnf(base, 2.0 * blend) : BlendLinearDodgef(base, 2.0 * (blend - 0.5)); }
float BlendScreenf(float base, float blend) { return 1.0 - (1.0 - base) * (1.0 - blend); }
float BlendOverlayf(float base, float blend) { return base < 0.5 ? 2.0 * base * blend : 1.0 - 2.0 * (1.0 - base) * (1.0 - blend); }
float BlendSoftLightf(float base, float blend) { return blend < 0.5 ? 2.0 * base * blend + base * base * (1.0 - 2.0 * blend) : sqrt(base) * (2.0 * blend - 1.0) + 2.0 * base * (1.0 - blend); }
float BlendColorDodgef(float base, float blend) { return blend == 1.0 ? blend : min(base / (1.0 - blend), 1.0); }
float BlendColorBurnf(float base, float blend) { return blend == 0.0 ? blend : max(1.0 - (1.0 - base) / blend, 0.0); }
float BlendVividLightf(float base, float blend) { return blend < 0.5 ? BlendColorBurnf(base, 2.0 * blend) : BlendColorDodgef(base, 2.0 * (blend - 0.5)); }
float BlendPinLightf(float base, float blend) { return blend < 0.5 ? BlendDarkenf(base, 2.0 * blend) : BlendLightenf(base, 2.0 * (blend - 0.5)); }
float BlendHardMixf(float base, float blend) { return BlendVividLightf(base, blend) < 0.5 ? 0.0 : 1.0; }
float BlendReflectf(float base, float blend) { return blend == 1.0 ? blend : min(base * base / (1.0 - blend), 1.0); }

vec3 BlendDarken(vec3 base, vec3 blend) { return min(blend, base); }
vec3 BlendMultiply(vec3 base, vec3 blend) { return base * blend; }
vec3 BlendColorBurn(vec3 base, vec3 blend) { return vec3(BlendColorBurnf(base.r, blend.r), BlendColorBurnf(base.g, blend.g), BlendColorBurnf(base.b, blend.b)); }
vec3 BlendSubtract(vec3 base, vec3 blend) { return max(base + blend - vec3(1.0), vec3(0.0)); }
vec3 BlendLighten(vec3 base, vec3 blend) { return max(blend, base); }
vec3 BlendScreen(vec3 base, vec3 blend) { return vec3(BlendScreenf(base.r, blend.r), BlendScreenf(base.g, blend.g), BlendScreenf(base.b, blend.b)); }
vec3 BlendColorDodge(vec3 base, vec3 blend) { return vec3(BlendColorDodgef(base.r, blend.r), BlendColorDodgef(base.g, blend.g), BlendColorDodgef(base.b, blend.b)); }
vec3 BlendAdd(vec3 base, vec3 blend) { return base + blend; }
vec3 BlendOverlay(vec3 base, vec3 blend) { return vec3(BlendOverlayf(base.r, blend.r), BlendOverlayf(base.g, blend.g), BlendOverlayf(base.b, blend.b)); }
vec3 BlendSoftLight(vec3 base, vec3 blend) { return vec3(BlendSoftLightf(base.r, blend.r), BlendSoftLightf(base.g, blend.g), BlendSoftLightf(base.b, blend.b)); }
vec3 BlendHardLight(vec3 base, vec3 blend) { return BlendOverlay(blend, base); }
vec3 BlendVividLight(vec3 base, vec3 blend) { return vec3(BlendVividLightf(base.r, blend.r), BlendVividLightf(base.g, blend.g), BlendVividLightf(base.b, blend.b)); }
vec3 BlendLinearLight(vec3 base, vec3 blend) { return vec3(BlendLinearLightf(base.r, blend.r), BlendLinearLightf(base.g, blend.g), BlendLinearLightf(base.b, blend.b)); }
vec3 BlendPinLight(vec3 base, vec3 blend) { return vec3(BlendPinLightf(base.r, blend.r), BlendPinLightf(base.g, blend.g), BlendPinLightf(base.b, blend.b)); }
vec3 BlendHardMix(vec3 base, vec3 blend) { return vec3(BlendHardMixf(base.r, blend.r), BlendHardMixf(base.g, blend.g), BlendHardMixf(base.b, blend.b)); }
vec3 BlendDifference(vec3 base, vec3 blend) { return abs(base - blend); }
vec3 BlendExclusion(vec3 base, vec3 blend) { return base + blend - 2.0 * base * blend; }
vec3 BlendAverage(vec3 base, vec3 blend) { return (base + blend) / 2.0; }
vec3 BlendNegation(vec3 base, vec3 blend) { return vec3(1.0) - abs(vec3(1.0) - base - blend); }
vec3 BlendReflect(vec3 base, vec3 blend) { return vec3(BlendReflectf(base.r, blend.r), BlendReflectf(base.g, blend.g), BlendReflectf(base.b, blend.b)); }
vec3 BlendGlow(vec3 base, vec3 blend) { return BlendReflect(blend, base); }
vec3 BlendPhoenix(vec3 base, vec3 blend) { return min(base, blend) - max(base, blend) + vec3(1.0); }
vec3 BlendLinearDodge(vec3 base, vec3 blend) { return min(base + blend, vec3(1.0)); }
vec3 BlendLinearBurn(vec3 base, vec3 blend) { return max(base + blend - vec3(1.0), vec3(0.0)); }
vec3 BlendTint(vec3 base, vec3 blend) { return vec3(max(base.x, max(base.y, base.z))) * blend; }

vec3 BlendHue(vec3 base, vec3 blend)
{
	vec3 baseHSL = RGBToHSL(base);
	return HSLToRGB(vec3(RGBToHSL(blend).r, baseHSL.g, baseHSL.b));
}

vec3 BlendSaturation(vec3 base, vec3 blend)
{
	vec3 baseHSL = RGBToHSL(base);
	return HSLToRGB(vec3(baseHSL.r, RGBToHSL(blend).g, baseHSL.b));
}

vec3 BlendColor(vec3 base, vec3 blend)
{
	vec3 blendHSL = RGBToHSL(blend);
	return HSLToRGB(vec3(blendHSL.r, blendHSL.g, RGBToHSL(base).b));
}

vec3 BlendLuminosity(vec3 base, vec3 blend)
{
	vec3 baseHSL = RGBToHSL(base);
	return HSLToRGB(vec3(baseHSL.r, baseHSL.g, RGBToHSL(blend).b));
}

vec3 ApplyBlending(int mode, vec3 colorA, vec3 colorB, float blend) {
    vec3 result = colorA;

    if (mode == 0) { result = mix(colorA, colorB, blend); }
    else if (mode == 1) { result = mix(colorA, BlendDarken(colorA, colorB), blend); }
    else if (mode == 2) { result = mix(colorA, BlendMultiply(colorA, colorB), blend); }
    else if (mode == 3) { result = mix(colorA, BlendColorBurn(colorA, colorB), blend); }
    else if (mode == 4) { result = mix(colorA, BlendSubtract(colorA, colorB), blend); }
    else if (mode == 5) { result = min(colorA, colorB); }
    else if (mode == 6) { result = mix(colorA, BlendLighten(colorA, colorB), blend); }
    else if (mode == 7) { result = mix(colorA, BlendScreen(colorA, colorB), blend); }
    else if (mode == 8) { result = mix(colorA, BlendColorDodge(colorA, colorB), blend); }
    else if (mode == 9) { result = mix(colorA, BlendAdd(colorA, colorB), blend); }
    else if (mode == 10) { result = max(colorA, colorB); }
    else if (mode == 11) { result = mix(colorA, BlendOverlay(colorA, colorB), blend); }
    else if (mode == 12) { result = mix(colorA, BlendSoftLight(colorA, colorB), blend); }
    else if (mode == 13) { result = mix(colorA, BlendHardLight(colorA, colorB), blend); }
    else if (mode == 14) { result = mix(colorA, BlendVividLight(colorA, colorB), blend); }
    else if (mode == 15) { result = mix(colorA, BlendLinearLight(colorA, colorB), blend); }
    else if (mode == 16) { result = mix(colorA, BlendPinLight(colorA, colorB), blend); }
    else if (mode == 17) { result = mix(colorA, BlendHardMix(colorA, colorB), blend); }
    else if (mode == 18) { result = mix(colorA, BlendDifference(colorA, colorB), blend); }
    else if (mode == 19) { result = mix(colorA, BlendExclusion(colorA, colorB), blend); }
    else if (mode == 20) { result = mix(colorA, BlendSubtract(colorA, colorB), blend); }
    else if (mode == 21) { result = mix(colorA, BlendReflect(colorA, colorB), blend); }
    else if (mode == 22) { result = mix(colorA, BlendGlow(colorA, colorB), blend); }
    else if (mode == 23) { result = mix(colorA, BlendPhoenix(colorA, colorB), blend); }
    else if (mode == 24) { result = mix(colorA, BlendAverage(colorA, colorB), blend); }
    else if (mode == 25) { result = mix(colorA, BlendNegation(colorA, colorB), blend); }
    else if (mode == 26) { result = mix(colorA, BlendHue(colorA, colorB), blend); }
    else if (mode == 27) { result = mix(colorA, BlendSaturation(colorA, colorB), blend); }
    else if (mode == 28) { result = mix(colorA, BlendColor(colorA, colorB), blend); }
    else if (mode == 29) { result = mix(colorA, BlendLuminosity(colorA, colorB), blend); }
    else if (mode == 30) { result = mix(colorA, BlendTint(colorA, colorB), blend); }
    else if (mode == 31) { result = colorA + colorB * blend; }
    else if (mode == 32) { result = mix(colorA, colorA + colorA * colorB, blend); }

    return result;
}
