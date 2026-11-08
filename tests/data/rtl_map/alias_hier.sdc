create_clock -name clk -period 10 [get_ports clk]
set_false_path -to [get_cells {v/s_reg[0]}]
set_false_path -to [get_cells {u/t[0]}]
