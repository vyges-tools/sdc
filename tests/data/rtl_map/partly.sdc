create_clock -name clk -period 10 [get_ports clk]
set_false_path -to [get_cells s9]
set_max_delay 3 -through [get_nets nx]
