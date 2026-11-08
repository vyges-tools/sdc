create_clock -name clk -period 10 [get_ports clk]
set_false_path -to [get_cells {r[10]}]
