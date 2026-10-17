/* Memory layout for the BL808 M0 Core (E907 - Wireless) */

MEMORY
{
  /* 
     XIP Flash Address. 
     Note: In a multi-core setup, the M0 firmware is usually packed 
     at a specific offset in the flash image. 
     The bootloader jumps to the entry point defined here.
  */
  FLASH (rx) : ORIGIN = 0x58000000, LENGTH = 16M

  /* 
     Wireless RAM (WRAM) - 112KB 
     Dedicated SRAM for the M0 core.
  */
  RAM (rwx)  : ORIGIN = 0x42030000, LENGTH = 112K
}

REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_HEAP", RAM);
REGION_ALIAS("REGION_STACK", RAM);