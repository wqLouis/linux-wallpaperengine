// [COMBO] {"material":"ui_editor_properties_blend_mode","combo":"BLENDMODE","type":"imageblending","default":31}

// [COMBO] {"material":"Axis","combo":"AXIS","type":"options","default":0,"options":{"Horizontal":0,"Vertical":1}}

#include "common.h"
#include "common_blending.h"


varying vec4 v_TexCoord;
uniform sampler2D g_Texture0; // {"material":"framebuffer","label":"ui_editor_properties_framebuffer","hidden":true}
uniform sampler2D g_Texture1; // {"combo":"MASK","default":"util/white","label":"ui_editor_properties_opacity_mask","material":"mask","mode":"opacitymask","paintdefaultcolor":"0 0 0 1"}

uniform float g_Time;

uniform vec3 u_Color1; // {"default":"1 0 0.2","material":"Color 1","type":"color"}
uniform vec3 u_Color2; // {"default":"0 0 1","material":"Color 2","type":"color"}
uniform float u_amount; // {"material":"Amount","default":1.5,"range":[0,100]}
uniform float u_Speed; // {"material":"Hue Speed","default":0,"range":[0,1]}
uniform float u_Oscillate; // {"material":"Oscillate","default":0,"range":[0,1]}
uniform float u_Opacity; // {"default":"1","material":"Opacity"}



void main() {
	float timer = sin(g_Time * u_Oscillate);
	vec4 scene = texSample2D(g_Texture0, v_TexCoord.xy);
	float mask = texSample2D(g_Texture1, v_TexCoord.zw).r;
	vec3 color1 = u_Color1;
	vec3 color2 = u_Color2;
#if AXIS
	float colorDistanceBlend = pow(v_TexCoord.y, u_amount);	
#else
	float colorDistanceBlend = pow(v_TexCoord.x, u_amount);			
#endif

	colorDistanceBlend += timer;
	
	vec3 resultColor = mix(color1, color2, colorDistanceBlend);
	resultColor.rgb = rgb2hsv(resultColor.rgb);
	resultColor.x = frac(resultColor.x + g_Time * u_Speed);
	resultColor.rgb = hsv2rgb(resultColor.rgb);

	//gl_FragColor = vec4(resultColor, 1.0);
vec3 finalColor = resultColor.rgb;

	// Apply blend mode
	finalColor = ApplyBlending(BLENDMODE, lerp(finalColor.rgb, scene.rgb, scene.a), finalColor.rgb, u_Opacity * mask);

float alpha = scene.a;

	gl_FragColor = vec4(finalColor,alpha);
}







