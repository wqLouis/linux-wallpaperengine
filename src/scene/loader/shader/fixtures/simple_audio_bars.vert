
// [COMBO] {"material":"ui_editor_properties_position","combo":"SHAPE","type":"options","default":0,"options":{"Bottom":0,"Top":1,"Left":2,"Right":3,"Circle - Inner":4,"Circle - Outer":5,"Center - L/R":6,"Center - U/D":7,"Stereo - L/R":8,"Stereo - U/D":9}}
// [COMBO] {"material":"ui_editor_properties_transform","combo":"TRANSFORM","type":"options","default":0}
// [COMBO] {"material":"ui_editor_properties_shape","combo":"BAR_STYLE","type":"options","default":1,"options":{"Rectangles":0,"Rounded Rectangles":1,"Smooth Curve":2}}
// [COMBO] {"material":"Deformity Correction","combo":"DEFORMITY","type":"options","default":3,"options":{"Ignore":0,"[legacy] Horizontal variation":1,"[legacy] Vertical variation":2,"Adaptive":3}}

#include "common.h"

uniform mat4 g_ModelViewProjectionMatrix;

uniform vec2 g_Offset; // {"default":"0 0","group":"ui_editor_properties_transform","label":"ui_editor_properties_offset","material":"offset"}
uniform vec2 g_Scale; // {"default":"1 1","group":"ui_editor_properties_transform","label":"ui_editor_properties_scale","material":"scale"}
uniform float g_Direction; // {"material":"angle","label":"ui_editor_properties_angle","default":0,"range":[0,6.28],"direction":true,"conversion":"rad2deg","group":"ui_editor_properties_transform"}
uniform vec4 g_Texture0Resolution;

attribute vec3 a_Position;
attribute vec2 a_TexCoord;

varying vec2 v_TexCoord;
varying vec2 p_TexCoord;
#if BAR_STYLE == 1
varying float i_DCorrectingFactor;
#endif

// Shape & Position
#define BOTTOM 0
#define TOP 1
#define LEFT 2
#define RIGHT 3
#define CIRCLE_INNER 4
#define CIRCLE_OUTER 5
#define CENTER_H 6
#define CENTER_V 7
#define STEREO_H 8
#define STEREO_V 9

vec2 applyFx(vec2 v) {
	v = rotateVec2(v - CAST2(0.5), -g_Direction);
	return (v + g_Offset) * 1 / g_Scale + CAST2(0.5);
}

void main() {
	p_TexCoord = a_TexCoord;
	v_TexCoord = a_TexCoord;
#if BAR_STYLE == 1
	i_DCorrectingFactor = 1;
#endif

#if DEFORMITY == 3
#if BAR_STYLE == 1
#if SHAPE == LEFT || SHAPE == RIGHT || SHAPE == CENTER_H || SHAPE == STEREO_H
	i_DCorrectingFactor = g_Texture0Resolution.y / g_Texture0Resolution.x;
#else
	i_DCorrectingFactor = g_Texture0Resolution.x / g_Texture0Resolution.y;
#endif
#endif
#endif

#if DEFORMITY == 1
	float xFactor = g_Texture0Resolution.x / g_Texture0Resolution.y;
	v_TexCoord.x *= xFactor;
	v_TexCoord.x += 0.5 - (0.5 * xFactor);
#endif
#if DEFORMITY == 2
	float yFactor = g_Texture0Resolution.y / g_Texture0Resolution.x;
	v_TexCoord.y *= yFactor;
	v_TexCoord.y += 0.5 - (0.5 * yFactor);
#endif

#endif

#if TRANSFORM
	v_TexCoord = applyFx(v_TexCoord);
#endif
	gl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);
}
