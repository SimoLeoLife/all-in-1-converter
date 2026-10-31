SET time_zone = '+00:00';
DROP DATABASE IF EXISTS arcturus_dbx;
CREATE DATABASE arcturus_dbx CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
USE arcturus_dbx;
CREATE TABLE items_base (
  id int(11) unsigned NOT NULL AUTO_INCREMENT,
  sprite_id int(11) NOT NULL DEFAULT 0,
  item_name varchar(70) NOT NULL DEFAULT '0',
  public_name varchar(56) NOT NULL DEFAULT '0',
  width int(11) NOT NULL DEFAULT 1,
  length int(11) NOT NULL DEFAULT 1,
  stack_height double(4,2) NOT NULL DEFAULT 0.00,
  allow_stack enum('0','1') NOT NULL DEFAULT '1',
  allow_sit enum('0','1') NOT NULL DEFAULT '0',
  allow_lay enum('0','1') NOT NULL DEFAULT '0',
  allow_walk enum('0','1') NOT NULL DEFAULT '0',
  type varchar(3) NOT NULL DEFAULT 's',
  interaction_type varchar(500) NOT NULL DEFAULT 'default',
  interaction_modes_count int(2) NOT NULL DEFAULT 1,
  vending_ids varchar(255) NOT NULL DEFAULT '0',
  multiheight varchar(50) DEFAULT '0',
  customparams varchar(5000) DEFAULT NULL,
  price decimal(12,4) DEFAULT NULL,
  ratio float DEFAULT NULL,
  flags bit(8) DEFAULT NULL,
  blobby mediumblob DEFAULT NULL,
  bin_id varbinary(16) DEFAULT NULL,
  created_at timestamp NULL DEFAULT NULL,
  day date DEFAULT NULL,
  json_data longtext DEFAULT NULL,
  PRIMARY KEY (id),
  KEY sprite_id (sprite_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE catalog_items (
  id int(11) NOT NULL AUTO_INCREMENT,
  item_ids varchar(666) NOT NULL,
  page_id int(11) NOT NULL,
  catalog_name varchar(100) NOT NULL,
  cost_credits int(11) NOT NULL DEFAULT 3,
  cost_points int(11) NOT NULL DEFAULT 0,
  points_type int(11) NOT NULL DEFAULT 0,
  amount int(11) NOT NULL DEFAULT 1,
  limited_stack int(11) NOT NULL DEFAULT 0,
  limited_sells int(11) NOT NULL DEFAULT 0,
  order_number int(11) NOT NULL DEFAULT 1,
  offer_id int(11) NOT NULL DEFAULT -1,
  song_id int(11) unsigned NOT NULL DEFAULT 0,
  extradata varchar(500) NOT NULL DEFAULT '',
  have_offer enum('0','1') NOT NULL DEFAULT '1',
  club_only enum('0','1') NOT NULL DEFAULT '0',
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
-- id 0 checks NO_AUTO_VALUE_ON_ZERO on restore
SET SESSION sql_mode = CONCAT(@@sql_mode, ',NO_AUTO_VALUE_ON_ZERO');
INSERT INTO items_base (id, sprite_id, item_name, public_name, stack_height, allow_sit, allow_walk, type, interaction_type, multiheight, customparams, price, ratio, flags, blobby, bin_id, created_at, day, json_data) VALUES
(0, 0, 'zero_id', 'Zero', 0.00, '0', '1', 's', 'default', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL),
(1, 3001, 'chair_norja', 'Chair \'Norja\' "quoted"', 1.10, '1', '0', 's', 'default', '0', 'back\\slash\\', 12.5000, 0.25, b'10100101', X'00FF1A0D0A5C27', X'DEADBEEF00', '2024-02-29 23:59:59', '2024-02-29', '{"a":"b\\"c","n":null}'),
(2, 3002, 'table_multi', 'Line1\nLine2\r\nLine3', 0.50, '0', '1', 'i', 'multiheight', '0;1.5;2', CONCAT('nul', CHAR(0), 'byte', CHAR(26), 'ctrlZ'), -0.0001, -1e-20, b'0', '', '', '1970-01-01 00:00:01', '1000-01-01', '[]'),
(3, 3003, 'unicode_☕', 'Ünïcødé 🪑 日本語 ‮rtl', 2.00, '1', '1', 'e', 'vendingmachine', NULL, '', 99999999.9999, 3.4e38, b'11111111', REPEAT(X'AB', 70000), X'00', NULL, NULL, NULL),
(4, 3004, 'backtick`name', '; DROP TABLE items_base; --', 0.01, '0', '0', 's', '%_like_\\%', 'x', '\'\'\'', 0, 0, NULL, X'', NULL, NULL, NULL, '');
INSERT INTO items_base (id, sprite_id, item_name, public_name) SELECT 100 + seq, 5000 + seq, CONCAT('bulk_', seq), CONCAT('Bulk \'', seq, '\'') FROM seq_1_to_1200;
INSERT INTO catalog_items (id, item_ids, page_id, catalog_name, offer_id, extradata, have_offer) VALUES
(1, '1', 10, 'chair_norja', 1001, '', '1'),
(2, '2;3', 10, 'table_multi', -1, 'extra\nwith\\escapes\'', '0'),
(3, '3', 11, 'unicode_☕', 70000, '🪑', '1'),
(4, '', 11, 'wallpaper_single_1', -1, '', '1');
INSERT INTO catalog_items (id, item_ids, page_id, catalog_name, offer_id) SELECT 100 + seq, 100 + seq, 20, CONCAT('bulk_', seq), seq FROM seq_1_to_1200;
