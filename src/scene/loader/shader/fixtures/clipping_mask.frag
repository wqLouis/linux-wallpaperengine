// [COMBO] {"material":"ui_editor_properties_blend_mode","combo":"BLENDMODE","type":"imageblending","default":0}
// [COMBO] {"material":"Clip to color","combo":"CLIPCOLOR","type":"options","default":0}
// [COMBO] {"material":"Invert mask","combo":"INVERT","type":"options","default":0}
// [COMBO] {"material":"Use parallax","combo":"PARALLAX","type":"options","default":0}
// [COMBO] {"material":"Texture alignment","combo":"ALIGNMENT","type":"options","default":0,"options":{"Fit":0,"Free":1}}

#include "common.h"
#include "common_blending.h"

uniform sampler2D g_Texture0; // {"material":"framebuffer","label":"ui_editor_properties_framebuffer","hidden":true}
uniform sampler2D g_Texture1; // {"default":"util/white","label":"Clip texture","material":"texture"}
uniform sampler2D g_Texture2; // {"combo":"MASK","default":"util/white","label":"ui_editor_properties_opacity_mask","material":"mask","mode":"opacitymask","paintdefaultcolor":"0 0 0 1"}
uniform vec4 g_Texture0Resolution;
uniform vec4 g_Texture1Resolution;
uniform vec2 u_textureOffset; // {"default":"0 0","linked":true,"material":"Texture offset","range":[-1,1]}
uniform vec2 u_textureScale; // {"default":"1 1","linked":true,"material":"Texture scale","range":[0,2]}
uniform vec2 u_maskOffset; // {"default":"0 0","linked":true,"material":"Mask offset","range":[-1,1]}
uniform vec2 u_maskScale; // {"default":"1 1","linked":true,"material":"Mask scale","range":[0,2]}
uniform vec2 u_texScaleCenter; // {"default":"0.5 0.5","material":"Texture scaling center","position":true,"range":[0,1]}
uniform vec2 u_maskScaleCenter; // {"default":"0.5 0.5","material":"Mask scaling center","position":true,"range":[0,1]}

uniform vec3 u_baseColor; // {"default":"1 1 1","material":"Color","type":"color"}
uniform float u_weight; // {"material":"Weight","default":1,"range":[0,5]}
uniform float u_threshold; // {"material":"Threshold (neutral: 0)","default":1,"range":[0,5]}
uniform float u_alpha; // {"material":"Opacity","default":1,"range":[0, 1]}
uniform vec2 u_textureDepth; // {"default":"1 1","linked":true,"material":"Texture parallax depth","range":[-2,2]}
uniform vec2 u_maskDepth; // {"default":"1 1","linked":true,"material":"Mask parallax depth","range":[-2,2]}

varying vec4 v_TexCoord;
varying vec2 v_ParallaxOffset;

#define ratioDiff_0 (g_Texture0Resolution.x / g_Texture0Resolution.y)
#define ratioDiff_1 (g_Texture1Resolution.x / g_Texture1Resolution.y)
#define ratioDiff vec2(ratioDiff_1, ratioDiff_0)
#define texScaleCenter (u_texScaleCenter * 2.0 - 1.0)
#define maskScaleCenter (u_maskScaleCenter * 2.0 - 1.0)

#if CLIPCOLOR
vec4 chromaKey(vec4 color, vec4 clip){
	vec3 hsv = rgb2hsv(color.rgb);
	vec3 target = rgb2hsv(u_baseColor);
	float dist = pow(length((target - hsv) / u_weight), float(u_threshold));
	float incrustation = 1.0 - clamp(3.0 * dist - 1.5, 0.0, 1.0);
	vec3 result = mix(color.rgb, clip.rgb, incrustation);
	float alpha = mix(color.a, clip.a, incrustation);
	return vec4(result, alpha);
}
#endif

void main() {

vec4 albedo = texSample2D(g_Texture0, v_TexCoord);
vec2 uvTex = ((v_TexCoord * 2.0 - 1.0 - texScaleCenter) / ratioDiff / u_textureScale + 1.0 + texScaleCenter) / 2.0 - u_textureOffset;
vec2 uvMask = ((v_TexCoord * 2.0 - 1.0 - maskScaleCenter) / u_maskScale + 1.0 + maskScaleCenter) / 2.0 - u_maskOffset;
#if ALIGNMENT == 1
#if PARALLAX == 1
vec4 clip = texSample2D(g_Texture1, uvTex + v_ParallaxOffset * u_textureDepth / u_textureScale);
float mask = texSample2D(g_Texture2, uvMask + v_ParallaxOffset * u_maskDepth / u_maskScale).r;
#else
vec4 clip = texSample2D(g_Texture1, uvTex);
float mask = texSample2D(g_Texture2, uvMask).r;
#endif
#else
#if PARALLAX == 1
vec4 clip = texSample2D(g_Texture1, v_TexCoord + v_ParallaxOffset * u_textureDepth);
float mask = texSample2D(g_Texture2, v_TexCoord + v_ParallaxOffset * u_maskDepth).r;
#else
vec4 clip = texSample2D(g_Texture1, v_TexCoord);
float mask = texSample2D(g_Texture2, v_TexCoord).r;
#endif
#endif

#if INVERT
mask = 1.0 - mask;
#endif

#if CLIPCOLOR
vec4 target = chromaKey(albedo, clip);
#else
vec4 target = clip;
#endif

	albedo.rgb = ApplyBlending(BLENDMODE, albedo.rgb, target.rgb, mask * albedo.a * u_alpha);
	gl_FragColor = albedo;
}