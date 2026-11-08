create_clock -name clk -period 10 [get_ports clk]
set_false_path -from [get_pins {r_reg[8]/Q}]
set_false_path -to [get_cells {r_reg[9]}]
set_false_path -from [get_cells {r_reg[8]/Q}]
