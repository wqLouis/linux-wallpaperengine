// [COMBO] {"material":"Mode","combo":"MODE","type":"options","default":0,"options":{"Mask":0,"Depth of field":1}}
// [COMBO] {"material":"ui_editor_properties_blend_mode","combo":"BLENDMODE","type":"imageblending","default":0}
// [COMBO] {"material":"Anamorphic lens","combo":"ANAMORPHIC","type":"options","default":0}
// [COMBO] {"material":"Visualize circle of confusion","combo":"COC","type":"options","default":0,"require":{"MODE":1}}

#include "common.h"
#include "common_blending.h"

uniform sampler2D g_Texture0; // {"hidden":true}
uniform sampler2D g_Texture1; // {"hidden":true}
uniform sampler2D g_Texture2; // {"hidden":true}
uniform float u_alpha; // {"material":"Opacity","default":1,"range":[0,1]}
uniform float u_aperture; // {"material":"Aperture","default":1,"range":[0,4],"group":"Lens"}
uniform float u_ratio; // {"material":"Ratio","default":2.39,"range":[1,4],"group":"Lens"}
uniform vec2 g_TexelSize;

varying vec2 v_TexCoord;
varying vec2 v_PixelSize;
varying float qualityNormalizer;

#if PRECISE
#define kernel 3
#else
#define kernel 2
#endif
#define blurPreciseLimit 0.6

void main() {
	vec4 albedo = texSample2D(g_Texture0, v_TexCoord);
	vec2 depthTex = texSample2D(g_Texture2, v_TexCoord.xy).rg;
	float depth = max(depthTex.x, depthTex.y) * u_aperture;

#if !COC
#if PRECISE
	depth *= (depth < blurPreciseLimit) * 6.0;
#if MODE == 0
	depth *= 0.2;
#endif
#else
#if MODE == 0
	depth *= 0.2 * qualityNormalizer;
#endif
	depth = clamp(0, 0.15, depth);
#endif

	if (depth > 0.01 && u_aperture > 0.01) {
		vec4 startAlbedo = albedo;
		vec2 offset = CAST2(0.0);
		vec2 pixelStep = depth * v_PixelSize;
		for (int i = -kernel; i <= kernel; i++){
#if VERTICAL
			offset.y = float(i) * pixelStep.y;
#else
			offset.x = float(i) * pixelStep.x;
#endif
			albedo.rgb += texSample2D(g_Texture0, v_TexCoord + offset).rgb;
		}
		albedo.rgb /= kernel + kernel + 2.0;
	}
#else
	albedo = vec4(depthTex + depthTex, 0, albedo.a);
#endif

#if VERTICAL
	vec4 baseAlbedo = texSample2D(g_Texture1, v_TexCoord);
	#if PRECISE
		albedo = vec4(ApplyBlending(BLENDMODE, baseAlbedo.rgb, albedo.rgb, u_alpha), albedo.a);
	#else
		albedo = vec4(ApplyBlending(BLENDMODE, baseAlbedo.rgb, albedo.rgb, u_alpha * saturate(depth * 9.0 * u_aperture)), albedo.a);
	#endif
#endif

	gl_FragColor = albedo;
}