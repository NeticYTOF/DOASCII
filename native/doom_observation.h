#ifndef DOASCII_DOOM_OBSERVATION_H
#define DOASCII_DOOM_OBSERVATION_H

#include <stdint.h>

#define DOASCII_MAX_VISIBLE_ENEMIES 128
#define DOASCII_LOGICAL_WIDTH 320
#define DOASCII_LOGICAL_HEIGHT 200

typedef struct {
    int x;
    int y;
    int width;
    int height;
} doascii_enemy_rect_t;

typedef struct {
    int player_state;
    int health;
    int armor;
    int weapon;
    int ammo;
    int32_t world_x;
    int32_t world_y;
    uint32_t angle;
    uint32_t game_tic;
    uint8_t forward_clearance;
    uint8_t left_clearance;
    uint8_t right_clearance;
    uint8_t back_clearance;
    int total_kills;
} doascii_player_status_t;

int DG_CopyVisibleEnemies(doascii_enemy_rect_t *output, int capacity);
void DG_CopyPlayerStatus(doascii_player_status_t *output);

#endif
