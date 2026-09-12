U0 DrawIt(CTask *task, CDC *dc)
{
  DCFill(dc,LTBLUE);
  dc->color=BLUE;
  GrLine3(dc,0,360,0,639,360,0,1,0);
  dc->color=GREEN;
  GrLine3(dc,0,361,0,639,479,0,1,0);
  dc->color=YELLOW;
  GrLine3(dc,0,479,0,639,361,0,1,0);
  dc->color=RED;
  GrLine3(dc,300,220,0,340,260,0,1,0);
  dc->color=WHITE;
  GrLine3(dc,340,220,0,300,260,0,1,0);
}

U0 Main()
{
  I64 ch=0,scan_code=0;
  SettingsPush(NULL,0);
  Fs->draw_it=&DrawIt;
  while (ch!=CH_ESC) {
    Refresh;
    ScanKey(&ch,&scan_code,FALSE);
    Sleep(8);
  }
  SettingsPop(NULL,0);
}

Main;
